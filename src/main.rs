
#![feature(phase)]

#[phase(plugin)]
extern crate regex_macros;
extern crate regex;
extern crate serialize;
extern crate term;

use std::io::fs::File;
use std::io::net::tcp::TcpStream;
use std::io::stdio::flush;
use std::io::timer::sleep;
use std::io::{BufferedReader, BufferedWriter, EndOfFile, IoResult, IoError, InvalidInput, BufWriter};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use term::{Terminal, WriterWrapper};
use term::stdout;

struct Bot {
    term: Box<Terminal<WriterWrapper> + Send>,
    tcp: TcpStream,
}
impl Bot {
    fn connect() -> TcpStream {
        loop {
            match TcpStream::connect("irc.fyrechat.net", 6667) {
                Ok(tcp) => return tcp,
                Err(e) => println!("Failed to connect: {}", e),
            }
        }
    }
    fn new() -> Arc<Mutex<Bot>> {
        Arc::new(Mutex::new(Bot {
            term: stdout().unwrap(),
            tcp: Bot::connect(),
        }))
    }
    fn run(bot: Arc<Mutex<Bot>>) -> IoResult<()> {
        let tcp = {
            let mut bot = bot.lock();
            try!(bot.send_nick("FTButt"));
            try!(bot.send_user());
            bot.tcp.clone()
        };
        Bot::read_loop(bot, BufferedReader::new(tcp))
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
    fn parse_args(line: &str) -> Vec<&str> {
        let reg = regex!(r" ([^: ]+)| :(.*)$");
        reg.captures_iter(line).map(|cap| {
            match cap.at(1) {
                "" => cap.at(2),
                x => x,
            }
        }).collect()
    }
    fn parse_line(line: &str) -> IoResult<(Option<&str>, &str, Vec<&str>)> {
        let reg = regex!(r"^(?::([^ ]+) )?([^ ]+)(.*)");
        let cap = match reg.captures(line) {
            Some(x) => x,
            None => return Err(IoError {
                kind: InvalidInput,
                desc: "Failed to parse line",
                detail: None,
            }),
        };
        let source = match cap.at(1) { "" => None, x => Some(x) };
        let command = cap.at(2);
        let args = Bot::parse_args(cap.at(3));
        Ok((source, command, args))
    }
    fn read_loop<T>(
        bot: Arc<Mutex<Bot>>, mut buf: T
    ) -> IoResult<()> where T: Reader + Buffer {
        loop {
            let line = try!(Bot::read_line(&mut buf));
            if line.is_empty() { continue }
            let line = String::from_utf8_lossy(line.as_slice());
            let (source, command, args) = try!(Bot::parse_line(line.as_slice()));
            try!(bot.lock().handle_command(source, command, args.as_slice()));
        }
    }
    fn handle_command(
        &mut self, source: Option<&str>, command: &str, args: &[&str]
    ) -> IoResult<()> {
        match command {
            "372" => { // MOTD
                match args {
                    [_, motd] => {
                        try!(self.term.fg(5));
                        println!("{}", motd);
                    },
                    _ => println!("CONFUSING MOTD: {}", args),
                }
            },
            "375" => {}, // Begin of MOTD
            "376" => { // End of MOTD
                try!(self.send_join("#vana"));
            },
            "422" => {
                try!(self.send_join("#vana"));
            }, // Motd is missing
            "433" => { // Nick already taken
                try!(self.send_nick("FTButtocks"));
            },
            "NOTICE" => {
                match args {
                    ["*", msg] => {
                        try!(self.term.fg(1));
                        print!("NOTICE: ");
                        flush();
                        try!(self.term.fg(3));
                        println!("{}", msg);
                    },
                    [chan, msg] => {
                        try!(self.term.fg(9));
                        print!("{} {} NOTICE: ", chan, source);
                        flush();
                        try!(self.term.fg(11));
                        println!("{}", msg);
                    },
                    _ => println!("CONFUSING NOTICE: {}", args),
                }
            }
            "PRIVMSG" => {
                match args {
                    [chan, msg] => {
                        try!(self.term.fg(9));
                        print!("{} {}: ", chan, source);
                        flush();
                        try!(self.term.fg(11));
                        println!("{}", msg);
                    },
                    _ => println!("CONFUSING PRIVMSG: {}", args),
                }

            },
            "PING" => {
                match args {
                    [msg] => try!(self.send_pong(Some(msg))),
                    [] => try!(self.send_pong(None)),
                    _ => println!("CONFUSING PING: {}", args),
                }

            },
            _ => {
                try!(self.term.fg(6));
                println!("{}, {}, {}", source, command, args);
            },
        }
        Ok(())
    }
    fn send(
        &mut self, source: Option<&str>, command: &str, args: &[&str], msg: Option<&str>
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
        self.tcp.write(bufdata.slice_to(pos as uint))
    }
    fn send_nick(&mut self, nick: &str) -> IoResult<()> {
        self.send(None, "NICK", [nick], None)
    }
    fn send_pong(&mut self, msg: Option<&str>) -> IoResult<()> {
        self.send(None, "PONG", [], msg)
    }
    fn send_user(&mut self) -> IoResult<()> {
        self.send(None, "USER", ["FTButt", "0", "*"], Some("FTButt"))
    }
    fn send_join(&mut self, chan: &str) -> IoResult<()> {
        self.send(None, "JOIN", [], Some(chan))
    }
}

fn main() {
    loop {
        let bot = Bot::new();
        println!("{}", Bot::run(bot));
    }
}
