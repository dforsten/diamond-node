//use hbbft::honey_badger::{self, MessageContent};
use hbbft::honey_badger::{self};
use parking_lot::RwLock;
use std::collections::VecDeque;

// use threshold_crypto::{SignatureShare};
use engines::hbbft::{sealing, NodeId};
// use hbbft::honey_badger::Message;
// use serde::{Deserialize, Serialize};
// use serde_json::{json, Result, Value};

use std::{
    fs::{self, create_dir_all, File},
    io::Write,
    path::PathBuf,
};
use std::thread::Thread;

pub type HbMessage = honey_badger::Message<NodeId>;

/**
Hbbft Message Process
// Broadcast - Echo
// Broadcast - Value
// Agreement
// Decryption Share
// Broadcast - CanDecode
// Subset - Message (proposer)
// Broadcast - Ready
// SignatureShare
//
//
*/

pub(crate) struct HbbftMessageMemorium {
    // future_messages_cache: BTreeMap<u64, Vec<(NodeId, HbMessage)>>,
    // signature_shares: BTreeMap<u64, Vec<(NodeId, HbMessage)>>,

    // decryption_shares: BTreeMap<u64, Vec<(NodeId, HbMessage)>>,
    //*
    // u64: epoch
    // NodeId: proposer
    // NodeId: node
    // HbMessage: message
    // */
    // agreements: BTreeMap<u64, Vec<(NodeId, NodeId, HbMessage)>>,
    message_tracking_id: u64,

    config_blocks_to_keep_on_disk: u64,

    last_block_deleted_from_disk: u64,

	dispatched_messages: RwLock<VecDeque<HbMessage>>,
	//thread: Option<std::thread::JoinHandle<Self>>,

	//sender: std::sync::mpsc::Sender<HbMessage>,
	//receiver: std::sync::mpsc::Receiver<HbMessage>,

	//memorial: std::sync::Arc<RwLock<HbbftMessageMemorium>>

}

pub(crate) struct HbbftMessageDispatcher {
	// dispatched_messages: RwLock<VecDeque<HbMessage>>,
	thread: Option<std::thread::JoinHandle<Self>>,
	memorial: std::sync::Arc<RwLock<HbbftMessageMemorium>>
}


impl HbbftMessageDispatcher {
	pub fn new() -> Self {
		HbbftMessageDispatcher {

			thread: None,
			memorial: std::sync::Arc::new(RwLock::new(HbbftMessageMemorium::new()))
		}
	}

	pub fn on_message_received(&mut self , message: &HbMessage) {
		//performance: dispatcher pattern + multithreading could improve performance a lot.

		let mut memorial = self.memorial.write();

		let mut lock = memorial.dispatched_messages.write();
		lock.push_back(message.clone());
		//self.sender.send(message.clone());

		if self.thread.is_none() {
			// let mut memo = self;
			// let mut arc = std::sync::Arc::new(&self);
			let arc_clone = self.memorial.clone();
			self.thread = Some(std::thread::spawn(move || {
				loop {

					let mut work_result = false;
					{
						let mut memorial = arc_clone.write();
						work_result = memorial.work_message();
					}

					if !work_result {
						std::thread::sleep(std::time::Duration::from_millis(250));
					}
				}
			}));
		}
	}

	pub fn free_memory(&mut self, _current_block: u64) {
		// TODO: make memorium freeing memory of ancient block.
	}
}



impl HbbftMessageMemorium {
    pub fn new() -> Self {

        HbbftMessageMemorium {
            // signature_shares: BTreeMap::new(),
            // decryption_shares: BTreeMap::new(),
            // agreements: BTreeMap::new(),
            message_tracking_id: 0,
            config_blocks_to_keep_on_disk: 200,
            last_block_deleted_from_disk: 0,
            dispatched_messages: RwLock::new(VecDeque::new()),
//			thread: None,
//			sender,
//			receiver
        }
    }

    pub fn on_message_string_received(&mut self, message_json: String, epoch: u64) {
        self.message_tracking_id += 1;

        //don't pick up messages if we do not keep any.
        // and don't pick up old delayed messages for blocks already
        // decided to not to keep.
        if self.config_blocks_to_keep_on_disk > 0 && epoch > self.last_block_deleted_from_disk {
            let mut path_buf = PathBuf::from(format!("data/messages/{}", epoch));
            if let Err(e) = create_dir_all(path_buf.as_path()) {
                warn!("Error creating key directory: {:?}", e);
                return;
            };

            path_buf.push(format!("{}.json", self.message_tracking_id));
            let path = path_buf.as_path();
            let mut file = match File::create(&path) {
                Ok(file) => file,
                Err(e) => {
                    warn!(target: "consensus", "Error creating hbbft memorial file: {:?}", e);
                    return;
                }
            };

            if let Err(e) = file.write(message_json.as_bytes()) {
                warn!(target: "consensus", "Error writing hbbft memorial file: {:?}", e);
            }

            //figure out if we have to delete a old block
            // 1. protect against integer underflow.
            // 2. block is so new, that we have to trigger a cleanup
            if epoch > self.config_blocks_to_keep_on_disk
                && epoch > self.last_block_deleted_from_disk + self.config_blocks_to_keep_on_disk
            {
                let paths = fs::read_dir("data/messages/").unwrap();

                for dir_entry_result in paths {
                    //println!("Name: {}", path.unwrap().path().display())

                    match dir_entry_result {
                        Ok(dir_entry) => {
                            let path_buf = dir_entry.path();

                            if path_buf.is_dir() {
                                let dir_name = path_buf.file_name().unwrap().to_str().unwrap();

                                match dir_name.parse::<u64>() {
                                    Ok(dir_epoch) => {
                                        if dir_epoch <= epoch - self.config_blocks_to_keep_on_disk {
                                            match fs::remove_dir_all(path_buf.clone()) {
                                                Ok(_) => {
                                                    info!(target: "consensus", "deleted old message directory: {:?}", path_buf);
                                                }
                                                Err(e) => {
                                                    warn!(target: "consensus", "could not delete old directories reason: {:?}", e);
                                                }
                                            }
                                        }
                                    }
                                    Err(_) => {}
                                }
                            }
                        }
                        Err(_) => {}
                    }
                    self.last_block_deleted_from_disk = epoch;
                }
            }
        }
    }


    fn work_message(&mut self ) -> bool {

        let mut message_option: Option<HbMessage> = None;

        {
            //scope it for short living.
            let mut lock =  self.dispatched_messages.write();
            message_option = lock.pop_front();
            //lock.len();
        }

        if let Some(message) = message_option {
            let epoch = message.epoch();
            match serde_json::to_string(&message) {
                Ok(json_string) => {
                    // debug!(target: "consensus", "{}", json_string);
                    self.on_message_string_received(json_string, epoch);
                }
                Err(e) => {
					// being unable to interprete a message, could result in consequences
					// not being able to report missbehavior,
					// or reporting missbehavior, where there was not a missbehavior.
                    error!(target: "consensus", "could not create json: {:?}", e);
                }
            }
			return true;
        }

		return false;

        // let content = message.content();

        //match content {
        //    MessageContent::Subset(subset) => {}

        //    MessageContent::DecryptionShare { proposer_id, share } => {
        // debug!("got decryption share from {} {:?}", proposer_id, share);

        //        if !self.decryption_shares.contains_key(&epoch) {
        //            match self.decryption_shares.insert(epoch, Vec::new()) {
        //                None => {}
        //                Some(vec) => {
        //                    //Vec<(NodeId, message)
        //                }
        //            }
        //        }
        //    }
        //}
    }

    pub fn on_sealing_message_received(&mut self, message: &sealing::Message, epoch: u64) {
        match serde_json::to_string(message) {
            Ok(json_string) => {
                // debug!(target: "consensus", "{}", json_string);

                self.on_message_string_received(json_string, epoch);
            }
            Err(e) => {
                error!(target: "consensus", "could not create json: {:?}", e);
            }
        }

        // todo: also remember sealing messages in an organized way
    }

    pub fn free_memory(&mut self, _current_block: u64) {
        // self.signature_shares.remove(&epoch);
    }
}
