// Copyright © 2014, Peter Atashian

extern crate googl;
extern crate irc;
extern crate mediawiki;
extern crate rustc_serialize;
extern crate url;

use irc::client::prelude::*;
use mediawiki::{Error as MwError, JsonFun, Mediawiki};
use rustc_serialize::json::{Json, decode};
use std::cmp::{max};
use std::fmt::{Display, Error as FmtError, Formatter};
use std::fs::{File, OpenOptions};
use std::io::{Read, Error as IoError, Write};
use std::sync::{Arc};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::{sleep, spawn};
use std::time::{Duration};
use url::{Url};
use url::form_urlencoded::{serialize};

#[derive(Debug)]
enum Error {
    Mediawiki(MwError),
    Io(IoError),
    Unknown,
}
impl From<MwError> for Error {
    fn from(err: MwError) -> Error {
        Error::Mediawiki(err)
    }
}
impl From<IoError> for Error {
    fn from(err: IoError) -> Error {
        Error::Io(err)
    }
}
fn main() {
    let (send, recv) = channel();
    spawn(move|| mw_thread(send));
    irc_thread(recv);
}
enum Change {
    Edit {
        user: String,
        title: String,
        comment: String,
        diff: i64,
        link: String,
    },
    New {
        user: String,
        title: String,
        comment: String,
        size: i64,
    },
}
impl Display for Change {
    fn fmt(&self, f: &mut Formatter) -> Result<(), FmtError> {
        struct Type<'a>(&'a str);
        impl<'a> Display for Type<'a> {
            fn fmt(&self, f: &mut Formatter) -> Result<(), FmtError> {
                write!(f, "[\x0307{}\x0f]", self.0)
            }
        }
        struct Comment<'a>(&'a str);
        impl<'a> Display for Comment<'a> {
            fn fmt(&self, f: &mut Formatter) -> Result<(), FmtError> {
                if self.0.is_empty() {
                    write!(f, "")
                } else {
                    write!(f, "(\x1d{}\x0f)", self.0)
                }
            }
        }
        struct User<'a>(&'a str);
        impl<'a> Display for User<'a> {
            fn fmt(&self, f: &mut Formatter) -> Result<(), FmtError> {
                write!(f, "\x0310{}\x0f", self.0)
            }
        }
        struct Title<'a>(&'a str);
        impl<'a> Display for Title<'a> {
            fn fmt(&self, f: &mut Formatter) -> Result<(), FmtError> {
                write!(f, "\x02{}\x0f", self.0)
            }
        }
        struct Diff(i64);
        impl Display for Diff {
            fn fmt(&self, f: &mut Formatter) -> Result<(), FmtError> {
                if self.0 >= 100 {
                    write!(f, "(\x02\x0303+{}\x0f)", self.0)
                } else if self.0 > 0 {
                    write!(f, "(\x0303+{}\x0f)", self.0)
                } else if self.0 <= -100 {
                    write!(f, "(\x02\x0304{}\x0f)", self.0)
                } else if self.0 < 0 {
                    write!(f, "(\x0304{}\x0f)", self.0)
                } else {
                    write!(f, "(\x0314{}\x0f)", self.0)
                }
            }
        }
        match self {
            &Change::New { ref user, ref title, ref comment, ref size } => {
                write!(f, "{} {} {} created {} {}", Type("new"),
                    Diff(*size), User(user), Title(title), Comment(comment))
            },
            &Change::Edit { ref user, ref title, ref comment, ref diff, ref link } => {
                write!(f, "{} {} {} edited {} {} {}", Type("edit"),
                    Diff(*diff), User(user), Title(title), Comment(comment), link)
            },
        }
    }
}
fn make_diff_link(title: &str, diff: &str, oldid: &str) -> String {
    let mut file = File::open("key.txt").unwrap();
    let mut key = String::new();
    file.read_to_string(&mut key).unwrap();
    let args = [("title", title), ("diff", diff), ("oldid", oldid)];
    let url = Url::parse(&format!("http://ftb.gamepedia.com/index.php?{}", serialize(args.iter().map(|&x| x)))).unwrap().serialize();
    let shortlink;
    loop {
        match googl::shorten(&key, &url) {
            Ok(link) => { shortlink = link; break },
            Err(_) => sleep(Duration::from_secs(5)),
        }
    }
    shortlink
}
fn process_change(send: &Sender<Change>, change: &Json) -> Result<(), Error> {
    let kind = try!(change.get("type").string());
    let user = try!(change.get("user").string()).to_owned();
    let title = try!(change.get("title").string()).to_owned();
    let comment = change.get("comment").string().unwrap_or("").to_owned();
    let oldlen = change.get("oldlen").integer().unwrap_or(0);
    let newlen = change.get("newlen").integer().unwrap_or(0);
    let revid = change.get("revid").integer().unwrap_or(0);
    let old_revid = change.get("old_revid").integer().unwrap_or(0);
    match kind {
        "edit" => send.send(Change::Edit {
            user: user,
            link: make_diff_link(&title, &revid.to_string(), &old_revid.to_string()),
            title: title,
            comment: comment,
            diff: newlen - oldlen,
        }).unwrap(),
        "new" => send.send(Change::New {
            user: user,
            title: title,
            comment: comment,
            size: newlen,
        }).unwrap(),
        _ => return Err(Error::Unknown),
    }
    Ok(())
}
fn mw_thread(send: Sender<Change>) {
    let mut file = File::open("ftb.json").unwrap();
    let mut s = String::new();
    file.read_to_string(&mut s).unwrap();
    let config = decode(&s).unwrap();
    let mw = Mediawiki::login(config).unwrap();
    let mut latest = 0;
    let mut rcfile = OpenOptions::new().write(true).append(true).open("rc.txt").unwrap();
    loop {
        let previous = latest;
        match mw.recent_changes() {
            Ok(rc) => for change in rc {
                match change {
                    Ok(change) => {
                        let id = change.get("rcid").integer().unwrap();
                        latest = max(id, latest);
                        if id <= previous || previous == 0 {
                            break
                        }
                        if let Err(e) = process_change(&send, &change) {
                            if let Error::Unknown = e {
                                writeln!(&mut rcfile, "{}", change).unwrap();
                            }
                            println!("{:?}", e);
                            break
                        }
                    },
                    Err(e) => {
                        println!("{:?}", e);
                        break
                    },
                }
                sleep(Duration::from_secs(1))
            },
            Err(e) => {
                println!("{:?}", e);
                continue;
            },
        };
        sleep(Duration::from_secs(10))
    }
}
fn irc_print_changes<T, U>(server: &Arc<IrcServer<T, U>>, recv: &Receiver<Change>) -> Result<(), Error>
    where T: IrcRead, U: IrcWrite {
    try!(server.identify());
    for change in recv {
        try!(server.send_privmsg("#FTB-Wiki-recentchanges", &change.to_string()));
    }
    Ok(())
}
fn irc_thread(recv: Receiver<Change>) -> ! {
    let server = Arc::new(IrcServer::new("irc.json").unwrap());
    let server_clone = server.clone();
    spawn(move|| irc_listen_thread(server_clone));
    loop {
        println!("Aw shit {:?}", irc_print_changes(&server, &recv));
        println!("Reconnecting {:?}", server.reconnect());
    }
}
fn irc_listen_thread<T, U>(server: Arc<IrcServer<T, U>>) where T: IrcRead, U: IrcWrite {
    for msg in server.iter() {

    }
}
