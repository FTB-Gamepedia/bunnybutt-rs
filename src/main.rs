// Copyright © 2014, Peter Atashian

#![feature(phase, slicing_syntax)]

extern crate cookie;
extern crate hyper;
extern crate irc;
#[phase(plugin)]
extern crate regex_macros;
extern crate regex;
extern crate serialize;
extern crate time;
extern crate url;

use cookie::Cookie;
use hyper::Url;
use hyper::client::request::Request;
use hyper::header::common::{
    Cookies,
    SetCookie,
    UserAgent,
};
use hyper::method::Method;
use hyper::net::Fresh;
use irc::data::config::Config;
use irc::server::{
    IrcServer,
    Server,
};
use irc::server::utils::Wrapper;
use serialize::json::{
    Json,
    decode,
};
use std::collections::HashMap;
use std::io::fs::File;
use std::io::timer::sleep;
use std::sync::Arc;
use std::time::Duration;
use time::{
    Tm,
    now_utc,
};
use url::form_urlencoded::serialize;

fn main() {
    let config = Config {
        owners: Vec::new(),
        nickname: "PonyButt".into_string(),
        username: "PonyButt".into_string(),
        realname: "PonyButt".into_string(),
        password: "".into_string(),
        server: "irc.fyrechat.net".into_string(),
        port: 6667,
        use_ssl: false,
        channels: vec!["#vana".into_string()],
        options: HashMap::new(),
    };
    let irc_server = Arc::new(IrcServer::from_config(config).unwrap());
    let server = Wrapper::new(&*irc_server);
    server.identify().unwrap();
    let api = WikiApi::login();
    let mut iter = irc_server.iter();
    let mut last = now_utc();
    loop {
        loop {
            let now = now_utc();
            if now.to_timespec() - last.to_timespec() > Duration::seconds(15) {
                break;
            }
            iter.next();
        }
        let now = now_utc();
        for change in api.get_changes(last, now).iter() {
            server.send_privmsg("#vana", change[]).unwrap();
            sleep(Duration::seconds(2));
        }
        last = now;
    }
}
fn make_url(args: &[(&str, &str)]) -> String {
    format!("http://ftb.gamepedia.com/api.php?{}", serialize(args.iter().map(|&x| x)))
}
struct WikiApi {
    cookies: Vec<Cookie>,
}
impl WikiApi {
    fn make_request(&self, url: &str, method: Method) -> Request<Fresh> {
        let mut request = Request::new(method, Url::parse(url).unwrap()).unwrap();
        request.headers_mut().set(UserAgent("PonyButt".into_string()));
        request.headers_mut().set(Cookies(self.cookies.clone()));
        request
    }
    fn login_first(username: &str, password: &str) -> (Vec<Cookie>, String) {
        #[deriving(Decodable, Show)]
        struct JsonLogin {
            login: JsonLoginInner,
        }
        #[deriving(Decodable, Show)]
        struct JsonLoginInner {
            result: String,
            token: String,
            cookieprefix: String,
            sessionid: String,
        }
        let url = make_url(&[("format", "json"), ("action", "login"), ("lgname", username), ("lgpassword", password)]);
        let mut request = Request::new(Method::Post, Url::parse(url[]).unwrap()).unwrap();
        request.headers_mut().set(UserAgent("PonyButt".into_string()));
        let mut response = request.start().unwrap().send().unwrap();
        let text = response.read_to_string().unwrap();
        let login = decode::<JsonLogin>(text[]).unwrap().login;
        let SetCookie(cookies) = response.headers.get::<SetCookie>().unwrap().clone();
        assert!(login.result[] == "NeedToken");
        (cookies, login.token)
    }
    fn login_final(&self, username: &str, password: &str, token: &str) {
        #[deriving(Decodable, Show)]
        struct JsonLoginFinal {
            login: JsonLoginFinalInner,
        }
        #[deriving(Decodable, Show)]
        struct JsonLoginFinalInner {
            result: String,
            lguserid: i32,
            lgusername: String,
            lgtoken: String,
            cookieprefix: String,
            sessionid: String,
        }
        let url = make_url(&[("format", "json"), ("action", "login"), ("lgname", username), ("lgpassword", password), ("lgtoken", token)]);
        let request = self.make_request(url[], Method::Post);
        let mut response = request.start().unwrap().send().unwrap();
        let text = response.read_to_string().unwrap();
        let login = decode::<JsonLoginFinal>(text[]).unwrap().login;
        assert!(login.result[] == "Success");
    }
    fn login() -> WikiApi {
        #[deriving(Decodable, Show)]
        struct LoginConfig {
            username: String,
            password: String,
        }
        let mut file = File::open(&Path::new("config.json")).unwrap();
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
    fn get_changes(&self, from: Tm, to: Tm) -> Vec<String> {
        // yyyymmddhhmmss
        let from = from.strftime("%Y%m%d%H%M%S").unwrap().to_string();
        let to = to.strftime("%Y%m%d%H%M%S").unwrap().to_string();
        let url = make_url(&[("format", "json"), ("action", "query"), ("list", "recentchanges"), ("rclimit", "5000"), ("rcprop", "title|user|parsedcomment|flags|sizes|loginfo"), ("rcdir", "newer"), ("rcstart", from[]), ("rcend", to[])]);
        let request = self.make_request(url[], Method::Get);
        let mut response = request.start().unwrap().send().unwrap();
        let text = response.read_to_string().unwrap();
        let json: Json = from_str(text[]).unwrap();
        let changes = json.find_path(&["query", "recentchanges"]).unwrap().as_array().unwrap();
        changes.iter().map(|change| {
            change.to_string()
        }).collect()
    }
}
