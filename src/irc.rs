// Copyright © 2014, Peter Atashian

use serialize::json::decode;
use std::cell::RefCell;
use std::io::fs::File;
use std::io::net::tcp::TcpStream;
use std::io::stdio::flush;
use std::io::{BufferedReader, EndOfFile, IoResult, IoError, InvalidInput, BufWriter, OtherIoError};
use std::sync::{Arc, Mutex};
use term::{Terminal, WriterWrapper};
use term::stdout;

#[deriving(Show)]
enum Source<'a> {
    ServerName(&'a str),
    ClientName(&'a str, &'a str, &'a str),
}
impl<'a> Source<'a> {
    fn shorten(self) -> &'a str {
        match self {
            ServerName(x) => x,
            ClientName(x, _, _) => x,
        }
    }
}
#[deriving(Decodable)]
struct Config {
    nickname: String,
    username: String,
    realname: String,
    password: String,
    server: String,
    port: u16,
    channels: Vec<String>,
}
impl Config {
    fn load() -> IoResult<Config> {
        let mut file = try!(File::open(&Path::new("config.json")));
        let data = try!(file.read_to_string());
        decode(data.as_slice()).map_err(|e| IoError {
            kind: InvalidInput,
            desc: "Decoder error",
            detail: Some(e.to_string()),
        })
    }
}
pub struct Bot {
    term: Box<Terminal<WriterWrapper> + Send>,
    tcp: RefCell<TcpStream>,
    config: Config,
}
impl Bot {
    pub fn new() -> IoResult<Arc<Mutex<Bot>>> {
        let config = try!(Config::load());
        let tcp = try!(TcpStream::connect(config.server.as_slice(), config.port));
        let term = try!(open_term());
        Ok(Arc::new(Mutex::new(Bot {
            term: term,
            tcp: RefCell::new(tcp),
            config: config,
        })))
    }
    pub fn run(bot: Arc<Mutex<Bot>>) -> IoResult<()> {
        let tcp = {
            let mut bot = bot.lock();
            try!(bot.send_nick());
            try!(bot.send_user());
            bot.tcp.borrow().clone()
        };
        Bot::read_loop(bot, BufferedReader::new(tcp))
    }
    fn read_loop<T>(
        bot: Arc<Mutex<Bot>>, mut buf: T
    ) -> IoResult<()> where T: Reader + Buffer {
        loop {
            let line = try!(read_line(&mut buf));
            if line.is_empty() { continue }
            let line = String::from_utf8_lossy(line.as_slice());
            let (source, command, args) = try!(parse_line(line.as_slice()));
            try!(bot.lock().handle_command(source, command, args.as_slice()));
        }
    }
    fn handle_command(
        &mut self, source: Source, command: &str, args: &[&str]
    ) -> IoResult<()> {
        match (command, args) {
            ("372", [_, motd]) => { // MOTD
                try!(self.term.fg(5));
                println!("{}", motd);
            },
            ("375", _) => {}, // Begin of MOTD
            ("376", _) => { // End of MOTD
                for chan in self.config.channels.iter() {
                    try!(self.send_join(chan.as_slice()));
                }
            },
            ("422", _) => { // Motd is missing
                for chan in self.config.channels.iter() {
                    try!(self.send_join(chan.as_slice()));
                }
            },
            ("433", _) => { // Nick already taken
                self.config.nickname.push_char('_');
                try!(self.send_nick());
            },
            ("NOTICE", [chan, msg]) => {
                try!(self.term.fg(1));
                print!("{} {} NOTICE: ", chan, source.shorten());
                flush();
                try!(self.term.fg(3));
                println!("{}", msg);
            },
            ("PRIVMSG", [chan, msg]) => {
                try!(self.term.fg(9));
                print!("{} {}: ", chan, source.shorten());
                flush();
                try!(self.term.fg(11));
                println!("{}", msg);
            },
            ("PING", [msg]) => {
                try!(self.send_pong(Some(msg)));
            },
            _ => {
                try!(self.term.fg(6));
                println!("{}, {}, {}", source.shorten(), command, args);
            },
        }
        Ok(())
    }
    fn send(
        &self, source: Option<&str>, command: &str, args: &[&str], msg: Option<&str>
    ) -> IoResult<()> {
        let mut bufdata = [0, ..512];
        let pos = {
            let mut buf = BufWriter::new(bufdata);
            match source {
                Some(source) => try!(write!(buf, ":{} ", source)),
                None => (),
            }
            try!(write!(buf, "{} ", command));
            for arg in args.iter() {
                try!(write!(buf, "{} ", arg));
            }
            match msg {
                Some(msg) => try!(write!(buf, ":{}", msg)),
                None => (),
            }
            try!(write!(buf, "\r\n"));
            try!(buf.tell())
        };
        self.tcp.borrow_mut().write(bufdata.slice_to(pos as uint))
    }
    fn send_join(&self, chan: &str) -> IoResult<()> {
        self.send(None, "JOIN", [], Some(chan))
    }
    fn send_nick(&self) -> IoResult<()> {
        self.send(None, "NICK", [self.config.nickname.as_slice()], None)
    }
    fn send_pong(&self, msg: Option<&str>) -> IoResult<()> {
        self.send(None, "PONG", [], msg)
    }
    fn send_user(&self) -> IoResult<()> {
        self.send(
            None, "USER", [self.config.username.as_slice(), "0", "*"],
            Some(self.config.realname.as_slice())
        )
    }
}
fn open_term() -> IoResult<Box<Terminal<WriterWrapper> + Send>> {
    match stdout() {
        Some(x) => Ok(x),
        None => return Err(IoError {
            kind: OtherIoError,
            desc: "Failed to open stdout terminal",
            detail: None,
        }),
    }
}
fn parse_args(line: &str) -> Vec<&str> {
    let reg = regex!(r" ([^: ]+)| :(.*)$");
    reg.captures_iter(line).map(|cap| {
        match cap.at(1) {
            "" => cap.at(2),
            x => x,
        }
    }).collect()
}
fn parse_source(line: &str) -> Source {
    let reg = regex!(r"(.*)!(.*)@(.*)");
    match reg.captures(line) {
        Some(x) => ClientName(x.at(1), x.at(2), x.at(3)),
        None => ServerName(line),
    }
}
fn parse_line(line: &str) -> IoResult<(Source, &str, Vec<&str>)> {
    let reg = regex!(r"^(?::([^ ]+) )?([^ ]+)(.*)");
    let cap = match reg.captures(line) {
        Some(x) => x,
        None => return Err(IoError {
            kind: InvalidInput,
            desc: "Failed to parse line",
            detail: None,
        }),
    };
    let source = parse_source(cap.at(1));
    let command = cap.at(2);
    let args = parse_args(cap.at(3));
    Ok((source, command, args))
}
// Borrowed from std::io::Buffer::read_until
fn read_line<T>(buf: &mut T) -> IoResult<Vec<u8>> where T: Reader + Buffer {
    let mut res = Vec::new();
    let mut used;
    loop {
        {
            let available = match buf.fill_buf() {
                Ok(n) => n,
                Err(ref e) if res.len() > 0 && e.kind == EndOfFile => {
                    used = 0;
                    break
                }
                Err(e) => return Err(e)
            };
            match available.iter().position(|&b| b == b'\n' || b == b'\r') {
                Some(i) => {
                    res.push_all(available.slice_to(i));
                    used = i + 1;
                    break
                }
                None => {
                    res.push_all(available);
                    used = available.len();
                }
            }
        }
        buf.consume(used);
    }
    buf.consume(used);
    Ok(res)
}
