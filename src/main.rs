// Copyright © 2014, Peter Atashian

#![feature(phase, slicing_syntax)]

extern crate cookie;
extern crate hyper;
extern crate irc;
#[phase(plugin)]
extern crate regex_macros;
extern crate regex;
extern crate "rustc-serialize" as rustc_serialize;
extern crate time;
extern crate url;

use cookie::Cookie;
use hyper::{HttpError, Url};
use hyper::client::request::Request;
use hyper::header::common::{
    Cookies,
    SetCookie,
    UserAgent,
};
use hyper::method::Method;
use hyper::net::Fresh;
use irc::server::{IrcServer, Server};
use irc::server::utils::Wrapper;
use rustc_serialize::json::{Json, decode};
use std::borrow::ToOwned;
use std::error::FromError;
use std::io::IoError;
use std::io::fs::File;
use std::io::timer::sleep;
use std::sync::Arc;
use std::thread::Thread;
use std::time::Duration;
use time::{
    ParseError,
    Tm,
    at_utc,
    now_utc,
};
use url::form_urlencoded::serialize;


fn main() {
    run_bot();
}
fn get_time() -> Tm {
    at_utc(now_utc().to_timespec() + Duration::seconds(-2))
}
fn run_bot() {
    let mut file = File::open(&Path::new("irc.json")).unwrap();
    let data = file.read_to_string().unwrap();
    let config = decode(data[]).unwrap();
    let irc_server = Arc::new(IrcServer::from_config(config).unwrap());
    let server = Wrapper::new(&*irc_server);
    server.identify().unwrap();
    let read_irc = irc_server.clone();
    Thread::spawn(move|| { read_irc.iter().count(); }).detach();
    let api = WikiApi::login();
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
fn make_url(args: &[(&str, &str)]) -> String {
    format!("http://ftb.gamepedia.com/api.php?{}", serialize(args.iter().map(|&x| x)))
}
#[deriving(Show)]
struct WikiError(String);
impl FromError<ParseError> for WikiError {
    fn from_error(err: ParseError) -> WikiError {
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
        WikiError(err.to_pretty_str())
    }
}
impl<'a> FromError<&'a str> for WikiError {
    fn from_error(err: &'a str) -> WikiError {
        WikiError(err.to_owned())
    }
}
struct WikiApi {
    cookies: Vec<Cookie>,
}
impl WikiApi {
    fn make_request(&self, url: &str, method: Method) -> Request<Fresh> {
        let mut request = Request::new(method, Url::parse(url).unwrap()).unwrap();
        request.headers_mut().set(UserAgent("PonyButt".to_owned()));
        request.headers_mut().set(Cookies(self.cookies.clone()));
        request
    }
    fn login_first(username: &str, password: &str) -> (Vec<Cookie>, String) {
        #[deriving(RustcDecodable, Show)]
        struct JsonLogin {
            login: JsonLoginInner,
        }
        #[deriving(RustcDecodable, Show)]
        struct JsonLoginInner {
            result: String,
            token: String,
            cookieprefix: String,
            sessionid: String,
        }
        let url = make_url(&[("format", "json"), ("action", "login"), ("lgname", username),
            ("lgpassword", password)]);
        let mut request = Request::new(Method::Post, Url::parse(url[]).unwrap()).unwrap();
        request.headers_mut().set(UserAgent("PonyButt".to_owned()));
        let mut response = request.start().unwrap().send().unwrap();
        let text = response.read_to_string().unwrap();
        let login = decode::<JsonLogin>(text[]).unwrap().login;
        let SetCookie(cookies) = response.headers.get::<SetCookie>().unwrap().clone();
        assert!(login.result[] == "NeedToken");
        (cookies, login.token)
    }
    fn login_final(&self, username: &str, password: &str, token: &str) {
        #[deriving(RustcDecodable, Show)]
        struct JsonLoginFinal {
            login: JsonLoginFinalInner,
        }
        #[deriving(RustcDecodable, Show)]
        struct JsonLoginFinalInner {
            result: String,
            lguserid: i32,
            lgusername: String,
            lgtoken: String,
            cookieprefix: String,
            sessionid: String,
        }
        let url = make_url(&[("format", "json"), ("action", "login"), ("lgname", username),
            ("lgpassword", password), ("lgtoken", token)]);
        let request = self.make_request(url[], Method::Post);
        let mut response = request.start().unwrap().send().unwrap();
        let text = response.read_to_string().unwrap();
        let login = decode::<JsonLoginFinal>(text[]).unwrap().login;
        assert!(login.result[] == "Success");
    }
    fn login() -> WikiApi {
        #[deriving(RustcDecodable, Show)]
        struct LoginConfig {
            username: String,
            password: String,
        }
        let mut file = File::open(&Path::new("ftb.json")).unwrap();
        let data = file.read_to_string().unwrap();
        let config: LoginConfig = decode(data[]).unwrap();
        let (cookies, token) = WikiApi::login_first(config.username[], config.password[]);
        let api = WikiApi {
            cookies: cookies,
        };
        api.login_final(config.username[], config.password[], token[]);
        println!("Logged in: {}", token);
        api
    }
    fn get_changes(&self, from: Tm, to: Tm) -> Result<Vec<Result<String, WikiError>>, WikiError> {
        // yyyymmddhhmmss
        let from = try!(from.strftime("%Y%m%d%H%M%S")).to_string();
        let to = try!(to.strftime("%Y%m%d%H%M%S")).to_string();
        let url = make_url(&[("format", "json"), ("action", "query"), ("list", "recentchanges"), ("rclimit", "5000"),
            ("rcprop", "user|userid|comment|parsedcomment|timestamp|title|ids|sha1|sizes|redirect|patrolled|loginfo|tags|flags"),
            ("rcdir", "newer"), ("rcstart", from[]), ("rcend", to[])]);
        let request = self.make_request(url[], Method::Get);
        let mut response = try!(request.start().and_then(|x| x.send()));
        let text = try!(response.read_to_string());
        let json: Json = try!(text[].parse().ok_or(text[]));
        let changes = try!(json.find_path(&["query", "recentchanges"]).and_then(|c| c.as_array())
            .ok_or(&json));
        Ok(changes.iter().map(|change| {
            let gets = |&:x: &str| change.find(x).and_then(|x| x.as_string()).ok_or(change);
            match try!(gets("type")) {
                "edit" => {
                    let comment = try!(gets("comment"));
                    let title = try!(gets("title"));
                    let user = try!(gets("user"));
                    let comment = if comment.is_empty() {
                        format!("– No edit summary")
                    } else {
                        format!("({})", comment)
                    };
                    Ok(format!("[\u{2}\u{3}03Edit\u{f}] \u{2}{}\u{f} – \u{2}{}\u{f} {}", title, user, comment))
                },
                "log" => {
                    let logtype = try!(gets("logtype"));
                    let logaction = try!(gets("logaction"));
                    match (logtype, logaction) {
                        ("tilesheet", "createtile") => {
                            let user = try!(gets("user"));
                            let item = try!(gets("item"));
                            let tmod = try!(gets("mod"));
                            Ok(format!("[\u{2}\u{3}03Tilesheet\u{f}] \u{2}{}\u{f} added \u{2}{}\u{f} from \u{2}{}\u{f}", user, item, tmod))
                        },
                        ("upload", "upload") => {
                            let title = try!(gets("title"));
                            let user = try!(gets("user"));
                            Ok(format!("[\u{2}\u{3}03Upload\u{f}] \u{2}{}\u{f} uploaded \u{2}{}\u{f}", user, title))
                        },
                        ("upload", "overwrite") => {
                            let title = try!(gets("title"));
                            let user = try!(gets("user"));
                            Ok(format!("[\u{2}\u{3}03Upload\u{f}] \u{2}{}\u{f} uploaded new version of \u{2}{}\u{f}", user, title))
                        },
                        _ => try!(Err(change)),
                    }
                },
                "new" => {
                    let comment = try!(gets("comment"));
                    let title = try!(gets("title"));
                    let user = try!(gets("user"));
                    let comment = if comment.is_empty() {
                        format!("– No edit summary")
                    } else {
                        format!("({})", comment)
                    };
                    Ok(format!("[\u{2}\u{3}03New\u{f}] \u{2}{}\u{f} – \u{2}{}\u{f} {}", title, user, comment))
                },
                _ => try!(Err(change)),
            }
        }).collect())
    }
}
