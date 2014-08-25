// Copyright © 2014, Peter Atashian

#![feature(phase)]

#[phase(plugin)]
extern crate regex_macros;
extern crate regex;
extern crate serialize;
extern crate term;

use std::io::IoResult;
use std::io::timer::sleep;
use std::time::Duration;

mod irc;

fn main() {
    fn try_again() -> IoResult<()> {
        let bot = try!(irc::Bot::new());
        irc::Bot::run(bot)
    }
    loop {
        println!("{}", try_again());
        sleep(Duration::seconds(10));
    }
}
