// Copyright © 2014, Peter Atashian

extern crate googl;
extern crate irc;
extern crate mediawiki;
extern crate rustc_serialize;

use irc::client::prelude::*;
use mediawiki::{Mediawiki};
use rustc_serialize::json::{decode};
use std::fs::{File};
use std::io::{Read};
use std::sync::{Arc};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::{sleep_ms, spawn};

enum Change {
    Edit {
        page: String,
        comment: String,
    },
}

fn main() {
    let (send, recv) = channel();
    spawn(move|| mw_thread(send));
    spawn(move|| irc_thread(recv));
    loop {
        sleep_ms(1000000)
    }
}
fn mw_thread(send: Sender<Change>) {
    let mut file = File::open("ftb.json").unwrap();
    let mut s = String::new();
    file.read_to_string(&mut s).unwrap();
    let config = decode(&s).unwrap();
    let mw = Mediawiki::login(config).unwrap();
    loop {
        let (changes, cont) = mw.get_rc().unwrap();
        // let change = Change::parse(
        sleep_ms(10000)
    }
}
fn irc_thread(recv: Receiver<Change>) {
    let server = Arc::new(IrcServer::new("irc.json").unwrap());
    server.identify().unwrap();
    let server_clone = server.clone();
    spawn(move|| irc_listen_thread(server_clone));
    for change in recv {
        
    }
}
fn irc_listen_thread<T, U>(server: Arc<IrcServer<T, U>>) where T: IrcRead, U: IrcWrite {
    for msg in server.iter() {
    }
}