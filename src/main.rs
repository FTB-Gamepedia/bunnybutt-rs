// Copyright © 2016, Peter Atashian

extern crate googl;
extern crate irc;
#[macro_use] extern crate lazy_static;
extern crate mediawiki;
extern crate rustc_serialize;
extern crate url;
extern crate regex;

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
use regex::Regex;

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
    New {
        user: String,
        title: String,
        comment: String,
        size: i64,
        link: String,
    },
    Edit {
        user: String,
        title: String,
        comment: String,
        diff: i64,
        link: String,
    },
    Delete {
        user: String,
        title: String,
        comment: String,
    },
    Upload {
        user: String,
        title: String,
        comment: String,
        link: String,
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
                    let rgx = Regex::new(r"\[\[(.+)\|(?P<t>{.+})\]\]");
                    let rgx1 = Regex::new(r"\[\[(?<t>{.+})\]\]");
                    let comment = rgx.replace_all(self.0, "$t");
                    comment = rgx1.replace_all(comment, "$t");
                    write!(f, "(\x1d{}\x0f)", comment)
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
                if self.0 >= 500 {
                    write!(f, "(\x02\x0303+{}\x0f)", self.0)
                } else if self.0 > 0 {
                    write!(f, "(\x0303+{}\x0f)", self.0)
                } else if self.0 <= -500 {
                    write!(f, "(\x02\x0304{}\x0f)", self.0)
                } else if self.0 < 0 {
                    write!(f, "(\x0304{}\x0f)", self.0)
                } else {
                    write!(f, "(\x0314{}\x0f)", self.0)
                }
            }
        }
        match self {
            &Change::New { ref user, ref title, ref comment, ref size, ref link } => {
                write!(f, "{} {} {} created {} {} {}", Type("new"),
                    Diff(*size), User(user), Title(title), Comment(comment), link)
            },
            &Change::Edit { ref user, ref title, ref comment, ref diff, ref link } => {
                write!(f, "{} {} {} edited {} {} {}", Type("edit"),
                    Diff(*diff), User(user), Title(title), Comment(comment), link)
            },
            &Change::Delete { ref user, ref title, ref comment } => {
                write!(f, "{} {} deleted {} {}", Type("delete"),
                    User(user), Title(title), Comment(comment))
            },
            &Change::Upload { ref user, ref title, ref comment, ref link } => {
                write!(f, "{} {} uploaded {} {} {}", Type("upload"),
                    User(user), Title(title), Comment(comment), link)
            },
        }
    }
}
fn shorten(link: &str) -> String {
    lazy_static! {
        static ref KEY: String = {
            let mut file = File::open("key.txt").unwrap();
            let mut key = String::new();
            file.read_to_string(&mut key).unwrap();
            key
        };
    }
    let shortlink;
    loop {
        match googl::shorten(&KEY, &link) {
            Ok(link) => { shortlink = link; break },
            Err(_) => sleep(Duration::from_secs(5)),
        }
    }
    shortlink
}
fn make_article_link(title: &str) -> String {
    let args = [("title", title)];
    let url = Url::parse(&format!("http://ftb.gamepedia.com/index.php?{}",
        serialize(args.iter().map(|&x| x)))).unwrap().serialize();
    shorten(&url)
}
fn make_revision_link(title: &str, oldid: &str) -> String {
    let args = [("title", title), ("oldid", oldid)];
    let url = Url::parse(&format!("http://ftb.gamepedia.com/index.php?{}",
        serialize(args.iter().map(|&x| x)))).unwrap().serialize();
    shorten(&url)
}
fn make_diff_link(title: &str, oldid: &str) -> String {
    let args = [("title", title), ("diff", "prev"), ("oldid", oldid)];
    let url = Url::parse(&format!("http://ftb.gamepedia.com/index.php?{}",
        serialize(args.iter().map(|&x| x)))).unwrap().serialize();
    shorten(&url)
}
fn process_change(send: &Sender<Change>, change: &Json) -> Result<(), Error> {
    let kind = try!(change.get("type").string());
    let user = try!(change.get("user").string()).to_owned();
    let title = try!(change.get("title").string()).to_owned();
    let comment = change.get("comment").string().unwrap_or("").to_owned();
    let oldlen = change.get("oldlen").integer().unwrap_or(0);
    let newlen = change.get("newlen").integer().unwrap_or(0);
    let revid = change.get("revid").integer().unwrap_or(0);
    let logaction = change.get("logaction").string();
    let logtype = change.get("logtype").string();
    match kind {
        "edit" => send.send(Change::Edit {
            user: user,
            link: make_diff_link(&title, &revid.to_string()),
            title: title,
            comment: comment,
            diff: newlen - oldlen,
        }).unwrap(),
        "new" => send.send(Change::New {
            user: user,
            link: make_revision_link(&title, &revid.to_string()),
            title: title,
            comment: comment,
            size: newlen,
        }).unwrap(),
        "log" => match (try!(logaction), try!(logtype)) {
            ("delete", "delete") => send.send(Change::Delete {
                user: user,
                title: title,
                comment: comment,
            }).unwrap(),
            ("upload", "upload") => send.send(Change::Upload {
                user: user,
                link: make_article_link(&title),
                title: title,
                comment: comment,
            }).unwrap(),
            _ => return Err(Error::Unknown),
        },
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
