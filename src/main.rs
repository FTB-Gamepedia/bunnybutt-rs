// Copyright © 2016, Peter Atashian

extern crate googl;
extern crate irc;
#[macro_use] extern crate lazy_static;
extern crate mediawiki;
extern crate serde;
extern crate serde_json;
extern crate url;

use irc::client::prelude::*;
use mediawiki::{Error as MwError, Mediawiki};
use serde_json::Value as Json;
use std::{
    cmp::max,
    fmt::{Display, Error as FmtError, Formatter},
    fs::{File, OpenOptions, rename},
    io::{Read, Error as IoError, Write},
    num::ParseIntError,
    sync::mpsc::{Receiver, Sender, channel},
    thread::{sleep, spawn},
    time::Duration,
};
use url::form_urlencoded::{Serializer};

#[derive(Debug)]
enum Error {
    Mediawiki(MwError),
    Io(IoError),
    ParseInt(ParseIntError),
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
impl From<ParseIntError> for Error {
    fn from(err: ParseIntError) -> Error {
        Error::ParseInt(err)
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
    EditInterwiki {
        user: String,
        comment: String,
        prefix: String,
        url: String,
        transclude: String,
        local: String,
    },
    DeleteInterwiki {
        user: String,
        prefix: String,
        comment: String,
    },
    NewInterwiki {
        user: String,
        comment: String,
        prefix: String,
        url: String,
        transclude: String,
        local: String,
    },
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
            &Change::EditInterwiki { ref user, ref comment, ref prefix, ref url, ref transclude, ref local } => {
                write!(f, "{} {} edited prefix {} ({}) (transclude: {}; local: {}) ({})", Type("interwiki"),
                    User(user), prefix, url, transclude, local, Comment(comment))
            },
            &Change::DeleteInterwiki { ref user, ref comment, ref prefix } => {
                write!(f, "{} {} deleted prefix {} ({})", Type("interwiki"),
                    User(user), prefix, Comment(comment))
            },
            &Change::NewInterwiki { ref user, ref comment, ref prefix, ref url, ref transclude, ref local } => {
                write!(f, "{} {} created new prefix {} ({}) (transclude: {}; local: {}) ({})",
                    Type("interwiki"), User(user), prefix, url, transclude, local, Comment(comment)
                )
            },
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
fn process_change(change: &Json) -> Result<Change, Error> {
    let kind = change["type"].as_str().unwrap_or("");
    let user = change["user"].as_str().unwrap_or("NO USER").to_owned();
    let title = change["title"].as_str().unwrap_or("NO TITLE").to_owned();
    let comment = change["comment"].as_str().unwrap_or("").to_owned();
    let oldlen = change["oldlen"].as_i64().unwrap_or(0);
    let newlen = change["newlen"].as_i64().unwrap_or(0);
    let revid = change["revid"].as_i64().unwrap_or(0);
    let logaction = change["logaction"].as_str().unwrap_or("");
    let logtype = change["logtype"].as_str().unwrap_or("");
    Ok(match kind {
        "edit" => Change::Edit {
            user: user,
            link: make_diff_link(&title, &revid.to_string()),
            title: title,
            comment: comment,
            diff: newlen - oldlen,
        },
        "new" => Change::New {
            user: user,
            link: make_revision_link(&title, &revid.to_string()),
            title: title,
            comment: comment,
            size: newlen,
        },
        "log" => match (logtype, logaction) {
            ("abusefilter", "modify") => Change::AbuseFilterModify {
                user: user,
                title: title,
            },
            ("block", "block") => Change::Block {
                user: user,
                title: title,
                duration: change["logparams"]["duration"].as_str().unwrap_or("").into(),
                comment: comment,
            },
            ("curseprofile", "comment-created") => Change::NewProfileComment {
                user: user,
                title: title,
            },
            ("curseprofile", "comment-edited") => Change::EditProfileComment {
                user: user,
                title: title,
            },
            ("curseprofile", "comment-replied") => Change::ReplyProfileComment {
                user: user,
                title: title,
            },
            ("delete", "delete") => Change::Delete {
                user: user,
                title: title,
                comment: comment,
            },
            ("delete", "restore") => Change::Restore {
                user: user,
                title: title,
                comment: comment,
            },
            ("move", "move") => Change::Move {
                user: user,
                title: title,
                newtitle: change["logparams"]["target_title"].as_str().unwrap_or("").into(),
                comment: comment,
            },
            ("move", "move_redir") => Change::MoveRedirect {
                user: user,
                title: title,
                newtitle: change["logparams"]["target_title"].as_str().unwrap_or("").into(),
                comment: comment,
            },
            ("newusers", "create") => Change::CreateUser {
                user: user,
            },
            ("pagetranslation", "mark") => Change::MarkTranslation {
                user: user,
                title: title,
            },
            ("protect", "modify") => Change::ModifyProtection {
                user: user,
                title: title,
                comment: comment,
                detail: change["0"].as_str().unwrap_or("").into(),
            },
            ("protect", "protect") => Change::AddProtection {
                user: user,
                title: title,
                comment: comment,
                detail: change["0"].as_str().unwrap_or("").into(),
            },
            ("protect", "unprotect") => Change::RemoveProtection {
                user: user,
                title: title,
                comment: comment,
            },
            ("tilesheet", "translatetile") => Change::TranslateTile {
                user: user,
                id: change["logparams"]["id"].as_i64().unwrap_or(0),
                name: change["logparams"]["name"].as_str().unwrap_or("").into(),
                desc: change["logparams"]["desc"].as_str().unwrap_or("").into(),
                lang: change["logparams"]["lang"].as_str().unwrap_or("").into(),
            },
            ("translationreview", "message") => Change::ReviewTranslation {
                user: user,
                title: title,
            },
            ("upload", "overwrite") => Change::UploadOverwrite {
                user: user,
                link: make_article_link(&title),
                title: title,
                comment: comment
            },
            ("upload", "upload") => Change::UploadNew {
                user: user,
                link: make_article_link(&title),
                title: title,
                comment: comment,
            },
            ("interwiki", "iw_edit") => Change::EditInterwiki {
                user: user,
                comment: comment,
                prefix: change["params"]["0"].as_str().unwrap_or("").into(),
                url: change["params"]["1"].as_str().unwrap_or("").into(),
                transclude: change["params"]["2"].as_str().unwrap_or("0").into(),
                local: change["params"]["3"].as_str().unwrap_or("0").into(),
            },
            ("interwiki", "iw_delete") => Change::DeleteInterwiki {
                user: user,
                comment: comment,
                prefix: change["params"]["0"].as_str().unwrap_or("").into(),
            },
            ("interwiki", "iw_add") => Change::NewInterwiki {
                user: user,
                comment: comment,
                prefix: change["params"]["0"].as_str().unwrap_or("").into(),
                url: change["params"]["1"].as_str().unwrap_or("").into(),
                transclude: change["params"]["2"].as_str().unwrap_or("0").into(),
                local: change["params"]["3"].as_str().unwrap_or("0").into(),
            },
            _ => return Err(Error::Unknown),
        },
        _ => return Err(Error::Unknown),
    })
}
fn mw_thread(send: Sender<Change>) {
    fn load_latest() -> Result<i64, Error> {
        let mut file = try!(File::open("latest.txt"));
        let mut s = String::new();
        try!(file.read_to_string(&mut s));
        Ok(try!(s.parse()))
    }
    fn save_latest(n: i64) -> Result<(), Error> {
        let mut file = try!(File::create("next.txt"));
        try!(write!(&mut file, "{}", n));
        drop(file);
        try!(rename("next.txt", "latest.txt"));
        Ok(())
    }
    let mw = Mediawiki::login_path("ftb.json").unwrap();
    let mut latest = load_latest().unwrap_or(0);
    println!("Resuming at {}", latest);
    let mut rcfile = OpenOptions::new().write(true).append(true).create(true).open("rc.txt").unwrap();
    let mut changes = Vec::new();
    loop {
        let previous = latest;
        for change in mw.query_recentchanges(20) {
            match change {
                Ok(change) => {
                    let id = change["rcid"].as_i64().unwrap();
                    latest = max(id, latest);
                    if id <= previous || previous == 0 {
                        break
                    }
                    writeln!(&mut rcfile, "{:#?}", change).unwrap();
                    match process_change(&change) {
                        Ok(change) => changes.push(change),
                        Err(Error::Unknown) => {
                            println!("{:?}", (change.get("type"), change.get("logaction"),
                                change.get("logtype")));
                        },
                        Err(e) => println!("{:?}", e),
                    }
                },
                Err(e) => {
                    println!("{:?}", e);
                    break
                },
            }
        };
        save_latest(latest).unwrap();
        for change in changes.drain(..).rev() {
            send.send(change).unwrap();
        }
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
    sleep(Duration::from_secs(8));
    for change in recv {
        if is_translation(&change) { continue }
        try!(server.send_privmsg("#FTB-Wiki-recentchanges", &change.to_string()));
        sleep(Duration::from_secs(1));
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
