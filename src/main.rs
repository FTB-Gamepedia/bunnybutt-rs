// Copyright © 2014, Peter Atashian

#![feature(phase, slicing_syntax)]

extern crate cookie;
extern crate hyper;
extern crate irc;
extern crate "rustc-serialize" as rustc_serialize;
extern crate time;
extern crate url;

use cookie::{CookieJar};
use hyper::{HttpError, Url};
use hyper::client::request::Request;
use hyper::client::response::Response;
use hyper::header::common::{Cookies, SetCookie, UserAgent};
use hyper::method::Method;
use hyper::status::StatusCode;
use irc::data::config::Config;
use irc::server::{IrcServer, Server};
use irc::server::utils::Wrapper;
use rustc_serialize::json::{Array, DecoderError, Json, ParserError};
use std::borrow::ToOwned;
use std::error::FromError;
use std::io::IoError;
use std::io::fs::File;
use std::io::timer::sleep;
use std::sync::Arc;
use std::thread::Thread;
use std::time::Duration;
use time::{Tm, at_utc, now_utc};
use time::ParseError as TimeError;
use url::form_urlencoded::serialize;
use url::ParseError as UrlError;

fn main() {
    run_bot();
}
fn get_time() -> Tm {
    at_utc(now_utc().to_timespec() + Duration::seconds(-2))
}
// FIXME - unable to infer enough type information about `_`; type annotations required
#[allow(unreachable_code)]
fn run_bot() {
    let config = Config::load(Path::new("irc.json")).unwrap();
    let irc_server = Arc::new(IrcServer::from_config(config).unwrap());
    irc_server.conn().set_keepalive(Some(60)).unwrap();
    let server = Wrapper::new(&*irc_server);
    server.identify().unwrap();
    let read_irc = irc_server.clone();
    Thread::spawn(move|| {
        let server = Wrapper::new(&*read_irc);
        let mut file = File::create(&Path::new("irc.txt")).unwrap();
        loop {
            for msg in read_irc.iter() {
                match msg {
                    Ok(msg) => write!(&mut file, "{}", msg.into_string()).unwrap(),
                    Err(e) => {
                        println!("IRC ERROR: {}", e);
                        break;
                    },
                }
            }
            read_irc.reconnect().unwrap();
            server.identify().unwrap();
        }
        () // FIXME - unable to infer enough type information about `_`; type annotations required
    }).detach();
    let mut api = WikiApi::new();
    api.login().unwrap();
    let mut last = get_time();
    loop {
        let now = get_time();
        match api.get_changes(last, now) {
            Ok(changes) => for change in changes.into_iter() {
                match change {
                    Ok(change) => {
                        server.send_privmsg("#FTB-Wiki-recentchanges", change[]).unwrap();
                        sleep(Duration::seconds(2));
                    },
                    Err(e) => println!("ERROR: {}", e),
                }
            },
            Err(e) => println!("SUPER ERROR: {}", e),
        }
        last = now;
        sleep(Duration::seconds(15));
    }
}
#[derive(Show)]
struct WikiError(String);
impl FromError<ParserError> for WikiError {
    fn from_error(err: ParserError) -> WikiError {
        WikiError(err.to_string())
    }
}
impl FromError<DecoderError> for WikiError {
    fn from_error(err: DecoderError) -> WikiError {
        WikiError(err.to_string())
    }
}
impl FromError<UrlError> for WikiError {
    fn from_error(err: UrlError) -> WikiError {
        WikiError(err.to_string())
    }
}
impl FromError<TimeError> for WikiError {
    fn from_error(err: TimeError) -> WikiError {
        WikiError(err.to_string())
    }
}
impl FromError<HttpError> for WikiError {
    fn from_error(err: HttpError) -> WikiError {
        WikiError(err.to_string())
    }
}
impl FromError<IoError> for WikiError {
    fn from_error(err: IoError) -> WikiError {
        WikiError(err.to_string())
    }
}
impl<'a> FromError<&'a Json> for WikiError {
    fn from_error(err: &'a Json) -> WikiError {
        WikiError(err.pretty().to_string())
    }
}
impl<'a> FromError<String> for WikiError {
    fn from_error(err: String) -> WikiError {
        WikiError(err)
    }
}
impl<'a> FromError<&'a str> for WikiError {
    fn from_error(err: &'a str) -> WikiError {
        WikiError(err.to_owned())
    }
}
trait JsonFun<'a> {
    fn get(self, &str) -> Result<&'a Json, WikiError>;
    fn string(self) -> Result<&'a str, WikiError>;
    fn array(self) -> Result<&'a Array, WikiError>;
    fn integer(self) -> Result<i64, WikiError>;
}
impl<'a> JsonFun<'a> for &'a Json {
    fn get(self, s: &str) -> Result<&'a Json, WikiError> {
        Ok(try!(self.find(s).ok_or(self)))
    }
    fn string(self) -> Result<&'a str, WikiError> {
        Ok(try!(self.as_string().ok_or(self)))
    }
    fn array(self) -> Result<&'a Array, WikiError> {
        Ok(try!(self.as_array().ok_or(self)))
    }
    fn integer(self) -> Result<i64, WikiError> {
        Ok(try!(self.as_i64().ok_or(self)))
    }
}
impl<'a> JsonFun<'a> for Result<&'a Json, WikiError> {
    fn get(self, s: &str) -> Result<&'a Json, WikiError> {
        self.and_then(|x| x.get(s))
    }
    fn string(self) -> Result<&'a str, WikiError> {
        self.and_then(|x| x.string())
    }
    fn array(self) -> Result<&'a Array, WikiError> {
        self.and_then(|x| x.array())
    }
    fn integer(self) -> Result<i64, WikiError> {
        self.and_then(|x| x.integer())
    }
}
struct WikiApi {
    baseurl: String,
    cookies: CookieJar<'static>,
    useragent: String,
    rclog: File,
}
impl WikiApi {
    fn make_url(&self, s: &str, args: &[(&str, &str)]) -> Result<Url, WikiError> {
        Ok(try!(Url::parse(format!("{}{}?{}", self.baseurl, s,
            serialize(args.iter().map(|&x| x)))[])))
    }
    fn make_timestamp(time: Tm) -> Result<String, WikiError> {
        Ok(try!(time.strftime("%Y%m%d%H%M%S")).to_string())
    }
    fn new() -> WikiApi {
        WikiApi {
            baseurl: "http://ftb.gamepedia.com/".to_owned(),
            cookies: CookieJar::new(&[]),
            useragent: "PonyButt".to_owned(),
            rclog: File::create(&Path::new("rc.txt")).unwrap(),
        }
    }
    fn request(&mut self, url: Url, method: Method) -> Result<Response, WikiError> {
        let mut request = try!(Request::new(method, url));
        request.headers_mut().set(UserAgent(self.useragent.clone()));
        request.headers_mut().set(Cookies::from_cookie_jar(&self.cookies));
        let response = try!(try!(request.start()).send());
        if let Some(cookies) = response.headers.get::<SetCookie>() {
            cookies.apply_to_cookie_jar(&mut self.cookies);
        }
        Ok(response)
    }
    fn login(&mut self) -> Result<(), WikiError> {
        let mut file = try!(File::open(&Path::new("ftb.json")));
        let text = try!(file.read_to_string());
        let config = try!(Json::from_str(text[]));
        let username = try!(config.get("username").string());
        let password = try!(config.get("password").string());
        self.do_login(username, password, None)
    }
    fn do_login(
        &mut self, username: &str, password: &str, token: Option<&str>,
    ) -> Result<(), WikiError> {
        let mut args = vec![("format", "json"), ("action", "login"), ("lgname", username),
            ("lgpassword", password)];
        if let Some(token) = token {
            args.push(("lgtoken", token));
        }
        let url = try!(self.make_url("api.php", args[]));
        let mut response = try!(self.request(url, Method::Post));
        if response.status != StatusCode::Ok {
            try!(Err(format!("Error while logging in: {}", response.status)));
        }
        let text = try!(response.read_to_string());
        let json: Json = try!(Json::from_str(text[]));
        let inner = try!(json.get("login"));
        let result = try!(inner.get("result").string());
        match result[] {
            "NeedToken" => self.do_login(username, password,
                Some(try!(inner.get("token").string()))),
            "Success" => {
                println!("Logged in to MediaWiki");
                Ok(())
            },
            _ => try!(Err(&json)),
        }
    }
    fn get_changes(&mut self, from: Tm, to: Tm) -> Result<Vec<Result<String, WikiError>>, WikiError> {
        let from = try!(WikiApi::make_timestamp(from));
        let to = try!(WikiApi::make_timestamp(to));
        let url = try!(self.make_url("api.php", &[("format", "json"), ("action", "query"),
            ("list", "recentchanges"), ("rclimit", "5000"),
            ("rcprop", "user|userid|comment|parsedcomment|timestamp|title|\
            ids|sha1|sizes|redirect|patrolled|loginfo|tags|flags"), ("rcdir", "newer"),
            ("rcstart", from[]), ("rcend", to[])]));
        let mut response = try!(self.request(url, Method::Get));
        if response.status != StatusCode::Ok {
            try!(Err(format!("Error while getting recent changes: {}", response.status)));
        }
        let text = try!(response.read_to_string());
        let json: Json = try!(Json::from_str(text[]));
        let changes = try!(json.get("query").get("recentchanges").array());
        Ok(changes.iter().map(|change| {
            try!(write!(&mut self.rclog, "{}", change.pretty()));
            let get_comment = |&:| -> Result<String, WikiError> {
                let comment = try!(change.get("comment").string());
                Ok(if comment.is_empty() {
                    format!("– No edit summary -")
                } else {
                    format!("({})", comment)
                })
            };
            match try!(change.get("type").string()) {
                "edit" => {
                    let comment = try!(get_comment());
                    let title = try!(change.get("title").string());
                    let user = try!(change.get("user").string());
                    let oldlen = try!(change.get("oldlen").integer());
                    let newlen = try!(change.get("newlen").integer());
                    let diff = if newlen > oldlen {
                        format!("(\u{3}03+{}\u{f})", newlen - oldlen)
                    } else if newlen < oldlen {
                        format!("(\u{3}04-{}\u{f})", oldlen - newlen)
                    } else {
                        format!("(\u{3}140\u{f})")
                    };
                    let old_revid = try!(change.get("old_revid").integer()).to_string();
                    let revid = try!(change.get("revid").integer()).to_string();
                    let link = try!(self.make_url("index.php", &[("title", title),
                        ("diff", revid[]), ("oldid", old_revid[])]));
                    Ok(format!("[\u{2}\u{3}03Edit\u{f}] \u{2}{}\u{f} {} \u{2}{}\u{f} {} {}", title,
                        diff, user, comment, link))
                },
                "new" => {
                    let comment = try!(get_comment());
                    let title = try!(change.get("title").string());
                    let user = try!(change.get("user").string());
                    Ok(format!("[\u{2}\u{3}03New\u{f}] \u{2}{}\u{f} – \u{2}{}\u{f} {}", title,
                        user, comment))
                },
                "log" => {
                    let logtype = try!(change.get("logtype").string());
                    let logaction = try!(change.get("logaction").string());
                    match (logtype, logaction) {
                        ("tilesheet", "createtile") => {
                            let user = try!(change.get("user").string());
                            let item = try!(change.get("item").string());
                            let tmod = try!(change.get("mod").string());
                            Ok(format!("[\u{2}\u{3}03Tilesheet\u{f}] \u{2}{}\u{f} added \
                                \u{2}{}\u{f} from \u{2}{}\u{f}", user, item, tmod))
                        },
                        ("oredict", "createentry") => {
                            let user = try!(change.get("user").string());
                            let item = try!(change.get("item").string());
                            let tmod = try!(change.get("mod").string());
                            let tag = try!(change.get("tag").string());
                            let flags = try!(change.get("flags").string());
                            Ok(format!("[\u{2}\u{3}03Oredict\u{f}] \u{2}{}\u{f} added \u{2}{}\u{f} \
                                 as \u{2}{}\u{f} from \u{2}{}\u{f} with flags \u{2}{}\u{f}", user,
                                 tag, item, tmod, flags))
                        },
                        ("oredict", "editentry") => {
                            let user = try!(change.get("user").string());
                            let item = try!(change.get("item").string());
                            let tmod = try!(change.get("mod").string());
                            let tag = try!(change.get("tag").string());
                            let diff = try!(change.get("diff").string());
                            Ok(format!("[\u{2}\u{3}03Oredict\u{f}] \u{2}{}\u{f} edited \
                                \u{2}{}\u{f} as \u{2}{}\u{f} from \u{2}{}\u{f} ({})", user, tag,
                                item, tmod, diff))
                        },
                        ("upload", "upload") => {
                            let title = try!(change.get("title").string());
                            let user = try!(change.get("user").string());
                            Ok(format!("[\u{2}\u{3}03Upload\u{f}] \u{2}{}\u{f} uploaded \
                                \u{2}{}\u{f}", user, title))
                        },
                        ("upload", "overwrite") => {
                            let title = try!(change.get("title").string());
                            let user = try!(change.get("user").string());
                            Ok(format!("[\u{2}\u{3}03Upload\u{f}] \u{2}{}\u{f} uploaded new \
                                version of \u{2}{}\u{f}", user, title))
                        },
                        ("delete", "delete") => {
                            let title = try!(change.get("title").string());
                            let user = try!(change.get("user").string());
                            Ok(format!("[\u{2}\u{3}03Delete\u{f}] \u{2}{}\u{f} deleted \
                                \u{2}{}\u{f}", user, title))
                        },
                        ("pagetranslation", "mark") => {
                            let title = try!(change.get("title").string());
                            let user = try!(change.get("user").string());
                            Ok(format!("[\u{2}\u{3}03Translation\u{f}] \u{2}{}\u{f} marked \
                                \u{2}{}\u{f} for translation", user, title))
                        },
                        ("newusers", "create") => {
                            let user = try!(change.get("user").string());
                            Ok(format!("[\u{2}\u{3}03New user\u{f}] New user \u{2}{}\u{f}", user))
                        },
                        ("move", "move") => {
                            let title = try!(change.get("title").string());
                            let user = try!(change.get("user").string());
                            let new_title = try!(change.get("move").get("new_title").string());
                            Ok(format!("[\u{2}\u{3}03Move\u{f}] \u{2}{}\u{f} moved \u{2}{}\u{f} to \
                                \u{2}{}\u{f}", user, title, new_title))
                        },
                        _ => try!(Err(change)),
                    }
                },
                _ => try!(Err(change)),
            }
        }).collect())
    }
}
