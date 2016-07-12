// Copyright © 2016, Peter Atashian

extern crate googl;
extern crate irc;
#[macro_use] extern crate lazy_static;
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
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::{sleep, spawn};
use std::time::{Duration};
use url::form_urlencoded::{Serializer};

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
        link: String,
    },
    // abusefilter
    AbuseFilterModify {
        user: String,
        title: String,
    },
    // block
    Block {
        user: String,
        title: String,
        duration: String,
        comment: String,
    },
    // curseprofile
    NewProfileComment {
        user: String,
        title: String
    },
    EditProfileComment {
        user: String,
        title: String
    },
    ReplyProfileComment {
        user: String,
        title: String
    },
    // delete
    Delete {
        user: String,
        title: String,
        comment: String,
    },
    Restore {
        user: String,
        title: String,
        comment: String,
    },
    // interwiki
    // move
    Move {
        user: String,
        title: String,
        newtitle: String,
        comment: String,
    },
    MoveRedirect {
        user: String,
        title: String,
        newtitle: String,
        comment: String,
    },
    // newusers
    CreateUser {
        user: String,
    },
    // oredict
    // pagetranslation
    MarkTranslation {
        user: String,
        title: String,
    },
    // protect
    AddProtection {
        user: String,
        title: String,
        comment: String,
        detail: String,
    },
    RemoveProtection {
        user: String,
        title: String,
        comment: String,
    },
    ModifyProtection {
        user: String,
        title: String,
        comment: String,
        detail: String,
    },
    // rights
    // tilesheet
    TranslateTile {
        user: String,
        id: i64,
        name: String,
        desc: String,
        lang: String,
    },
    // translationreview
    ReviewTranslation {
        user: String,
        title: String,
    },
    // upload
    UploadOverwrite {
        user: String,
        title: String,
        comment: String,
        link: String,
    },
    UploadNew {
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
            &Change::Edit { ref user, ref title, ref comment, ref diff, ref link } => {
                write!(f, "{} {} {} edited {} {} {}", Type("edit"),
                    Diff(*diff), User(user), Title(title), Comment(comment), link)
            },
            &Change::New { ref user, ref title, ref comment, ref size, ref link } => {
                write!(f, "{} {} {} created {} {} {}", Type("new"),
                    Diff(*size), User(user), Title(title), Comment(comment), link)
            },
            // abusefilter
            &Change::AbuseFilterModify { ref user, ref title } => {
                write!(f, "{} {} modified the abuse filter {}", Type("abusefilter"),
                    User(user), Title(title))
            },
            // block
            &Change::Block { ref user, ref title, ref duration, ref comment } => {
                write!(f, "{} {} blocked {} for {} {}", Type("block"),
                    User(user), Title(title), duration, Comment(comment))
            },
            // curseprofile
            &Change::NewProfileComment { ref user, ref title } => {
                write!(f, "{} {} created a new comment on {}", Type("curseprofile"),
                    User(user), Title(title))
            },
            &Change::EditProfileComment { ref user, ref title } => {
                write!(f, "{} {} edited a profile comment on {}", Type("curseprofile"),
                    User(user), Title(title))
            },
            &Change::ReplyProfileComment { ref user, ref title } => {
                write!(f, "{} {} replied to a comment on {}", Type("curseprofile"),
                    User(user), Title(title))
            },
            // delete
            &Change::Delete { ref user, ref title, ref comment } => {
                write!(f, "{} {} deleted {} {}", Type("delete"),
                    User(user), Title(title), Comment(comment))
            },
            &Change::Restore { ref user, ref title, ref comment } => {
                write!(f, "{} {} restored {} {}", Type("delete"),
                    User(user), Title(title), Comment(comment))
            },
            // interwiki
            // move
            &Change::Move { ref user, ref title, ref newtitle, ref comment } => {
                write!(f, "{} {} moved {} to {} {}", Type("move"),
                    User(user), Title(title), Title(newtitle), Comment(comment))
            },
            &Change::MoveRedirect { ref user, ref title, ref newtitle, ref comment } => {
                write!(f, "{} {} moved {} to {} overwriting a redirect {}", Type("move"),
                    User(user), Title(title), Title(newtitle), Comment(comment))
            },
            // newusers
            &Change::CreateUser { ref user } => {
                write!(f, "{} {} created an account", Type("user"),
                    User(user))
            },
            // oredict
            // pagetranslation
            &Change::MarkTranslation { ref user, ref title } => {
                write!(f, "{} {} marked {} for translation", Type("mark"),
                    User(user), Title(title))
            },
            // protect
            &Change::ModifyProtection { ref user, ref title, ref comment, ref detail } => {
                write!(f, "{} {} modified the protection for {} {} {}", Type("protect"),
                    User(user), Title(title), detail, Comment(comment))
            },
            &Change::AddProtection { ref user, ref title, ref comment, ref detail } => {
                write!(f, "{} {} added protection to {} {} {}", Type("protect"),
                    User(user), Title(title), detail, Comment(comment))
            },
            &Change::RemoveProtection { ref user, ref title, ref comment } => {
                write!(f, "{} {} removed the protection on {} {}", Type("protect"),
                    User(user), Title(title), Comment(comment))
            },
            // rights
            // tilesheet
            &Change::TranslateTile { ref user, ref id, ref name, ref desc, ref lang } => {
                write!(f, "{} {} translated tile #{} to {} as {} {}", Type("tilesheet"),
                    User(user), id, Title(lang), Title(name), Comment(desc))
            },
            // translationreview
            &Change::ReviewTranslation { ref user, ref title } => {
                write!(f, "{} {} reviewed the translation {}", Type("review"),
                    User(user), Title(title))
            },
            // upload
            &Change::UploadOverwrite { ref user, ref title, ref comment, ref link } => {
                write!(f, "{} {} uploaded a new version of {} {} {}", Type("upload"),
                    User(user), Title(title), Comment(comment), link)
            },
            &Change::UploadNew { ref user, ref title, ref comment, ref link } => {
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
    let args = &[("title", title)];
    let query = Serializer::new(String::new()).extend_pairs(args).finish();
    let url = &format!("http://ftb.gamepedia.com/index.php?{}", query);
    shorten(&url)
}
fn make_revision_link(title: &str, oldid: &str) -> String {
    let args = &[("title", title), ("oldid", oldid)];
    let query = Serializer::new(String::new()).extend_pairs(args).finish();
    let url = &format!("http://ftb.gamepedia.com/index.php?{}", query);
    shorten(&url)
}
fn make_diff_link(title: &str, oldid: &str) -> String {
    let args = &[("title", title), ("diff", "prev"), ("oldid", oldid)];
    let query = Serializer::new(String::new()).extend_pairs(args).finish();
    let url = &format!("http://ftb.gamepedia.com/index.php?{}", query);
    shorten(&url)
}
fn process_change(send: &Sender<Change>, change: &Json) -> Result<(), Error> {
    let kind = change.get("type").string().unwrap_or("");
    let user = change.get("user").string().unwrap_or("NO USER").to_owned();
    let title = change.get("title").string().unwrap_or("NO TITLE").to_owned();
    let comment = change.get("comment").string().unwrap_or("").to_owned();
    let oldlen = change.get("oldlen").integer().unwrap_or(0);
    let newlen = change.get("newlen").integer().unwrap_or(0);
    let revid = change.get("revid").integer().unwrap_or(0);
    let logaction = change.get("logaction").string().unwrap_or("");
    let logtype = change.get("logtype").string().unwrap_or("");
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
        "log" => match (logtype, logaction) {
            ("abusefilter", "modify") => send.send(Change::AbuseFilterModify {
                user: user,
                title: title,
            }).unwrap(),
            ("block", "block") => send.send(Change::Block {
                user: user,
                title: title,
                duration: change.get("logparams").get("duration").string().unwrap_or("").into(),
                comment: comment,
            }).unwrap(),
            ("curseprofile", "comment-created") => send.send(Change::NewProfileComment {
                user: user,
                title: title,
            }).unwrap(),
            ("curseprofile", "comment-edited") => send.send(Change::EditProfileComment {
                user: user,
                title: title,
            }).unwrap(),
            ("curseprofile", "comment-replied") => send.send(Change::ReplyProfileComment {
                user: user,
                title: title,
            }).unwrap(),
            ("delete", "delete") => send.send(Change::Delete {
                user: user,
                title: title,
                comment: comment,
            }).unwrap(),
            ("delete", "restore") => send.send(Change::Restore {
                user: user,
                title: title,
                comment: comment,
            }).unwrap(),
            ("move", "move") => send.send(Change::Move {
                user: user,
                title: title,
                newtitle: change.get("logparams").get("target_title").string().unwrap_or("").into(),
                comment: comment,
            }).unwrap(),
            ("move", "move_redir") => send.send(Change::MoveRedirect {
                user: user,
                title: title,
                newtitle: change.get("logparams").get("target_title").string().unwrap_or("").into(),
                comment: comment,
            }).unwrap(),
            ("newusers", "create") => send.send(Change::CreateUser {
                user: user,
            }).unwrap(),
            ("pagetranslation", "mark") => send.send(Change::MarkTranslation {
                user: user,
                title: title,
            }).unwrap(),
            ("protect", "modify") => send.send(Change::ModifyProtection {
                user: user,
                title: title,
                comment: comment,
                detail: change.get("0").string().unwrap_or("").into(),
            }).unwrap(),
            ("protect", "protect") => send.send(Change::AddProtection {
                user: user,
                title: title,
                comment: comment,
                detail: change.get("0").string().unwrap_or("").into(),
            }).unwrap(),
            ("protect", "unprotect") => send.send(Change::RemoveProtection {
                user: user,
                title: title,
                comment: comment,
            }).unwrap(),
            ("tilesheet", "translatetile") => send.send(Change::TranslateTile {
                user: user,
                id: change.get("logparams").get("id").integer().unwrap_or(0),
                name: change.get("logparams").get("name").string().unwrap_or("").into(),
                desc: change.get("logparams").get("desc").string().unwrap_or("").into(),
                lang: change.get("logparams").get("lang").string().unwrap_or("").into(),
            }).unwrap(),
            ("translationreview", "message") => send.send(Change::ReviewTranslation {
                user: user,
                title: title,
            }).unwrap(),
            ("upload", "overwrite") => send.send(Change::UploadOverwrite {
                user: user,
                link: make_article_link(&title),
                title: title,
                comment: comment
            }).unwrap(),
            ("upload", "upload") => send.send(Change::UploadNew {
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
        for change in mw.query_recentchanges(10) {
            match change {
                Ok(change) => {
                    let id = change.get("rcid").integer().unwrap();
                    latest = max(id, latest);
                    if id <= previous || previous == 0 {
                        break
                    }
                    if let Err(e) = process_change(&send, &change) {
                        writeln!(&mut rcfile, "{}", change.pretty()).unwrap();
                        println!("{:?}", e);
                    }
                },
                Err(e) => {
                    println!("{:?}", e);
                    break
                },
            }
        };
        sleep(Duration::from_secs(10))
    }
}
fn is_translation(change: &Change) -> bool {
    if let &Change::New { ref title, .. } = change {
        title.starts_with("Translations:")
    } else if let &Change::Edit { ref title, .. } = change {
        title.starts_with("Translations:")
    } else { false }
}
fn irc_print_changes(server: &IrcServer, recv: &Receiver<Change>) -> Result<(), Error> {
    try!(server.identify());
    for change in recv {
        if is_translation(&change) { continue }
        try!(server.send_privmsg("#FTB-Wiki-recentchanges", &change.to_string()));
        sleep(Duration::from_secs(1))
    }
    Ok(())
}
fn irc_thread(recv: Receiver<Change>) -> ! {
    let server = IrcServer::new("irc.json").unwrap();
    let server_clone = server.clone();
    spawn(move|| irc_listen_thread(server_clone));
    loop {
        println!("Aw shit {:?}", irc_print_changes(&server, &recv));
        println!("Reconnecting {:?}", server.reconnect());
    }
}
fn irc_listen_thread(server: IrcServer) {
    let mut file = File::create("irc.txt").unwrap();
    loop {
        for msg in server.iter() {
            writeln!(&mut file, "{:?}", msg).unwrap();
        }
        sleep(Duration::from_secs(1))
    }
}
