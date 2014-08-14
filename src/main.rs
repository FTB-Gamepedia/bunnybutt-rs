#![feature(phase)]
#[phase(plugin)]
extern crate regex_macros;
extern crate regex;
extern crate term;

use std::io::net::tcp::TcpStream;
use std::io::stdio::flush;
use std::io::timer::sleep;
use std::io::{BufferedReader, BufferedWriter, EndOfFile, IoResult};
use std::time::Duration;
use term::{Terminal, WriterWrapper};
use term::stdout;

struct Bot {
term: Box<Terminal<WriterWrapper> + Send>,
    cout: BufferedWriter<TcpStream>,
    cin: BufferedReader<TcpStream>,
}
impl Bot {
    fn connect() -> TcpStream {
        loop {
            match TcpStream::connect("irc.esper.net", 6667) {
                Ok(tcp) => return tcp,
                Err(e) => println!("Failed to connect: {}", e),
            }
        }
    }
    fn new() -> Bot {
        let tcp = Bot::connect();
        println!("Connected!");
        Bot {
            term: stdout().unwrap(),
            cout: BufferedWriter::new(tcp.clone()),
            cin: BufferedReader::new(tcp),
        }
    }
    // Borrowed from std::io::Buffer::read_until
    fn read_line(&mut self) -> IoResult<Vec<u8>> {
        let mut res = Vec::new();
        let mut used;
        loop {
            {
                let available = match self.cin.fill_buf() {
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
            self.cin.consume(used);
        }
        self.cin.consume(used);
        Ok(res)
    }
    fn parse_line(line: &str) -> (Option<&str>, Vec<&str>, Option<&str>) {
        let reg_source = regex!(r"^:([^ ]+)");
        let reg_msg = regex!(r" :(.*)$");
        let reg_param = regex!(r"([^ ]+)");
        let cap_source = reg_source.captures(line);
        let cap_source = cap_source.as_ref();
        let source = cap_source.map(|cap| cap.at(1));
        let cap_msg = reg_msg.captures(line);
        let cap_msg = cap_msg.as_ref();
        let msg = cap_msg.map(|cap| cap.at(1));
        let begin = cap_source.and_then(|cap| cap.pos(0)
            .map(|pos| pos.val1())).unwrap_or(0);
        let end = cap_msg.and_then(|cap| cap.pos(0)
            .map(|pos| pos.val0())).unwrap_or(line.len());
        let params = reg_param.captures_iter(line.slice(begin, end))
            .map(|cap| cap.at(1)).collect::<Vec<&str>>();
        (source, params, msg)
    }
    fn shorten(name: &str) -> &str {
        match name.find('!') {
            Some(pos) => name.slice_to(pos),
            None => name,
        }
    }
    fn run(&mut self) -> IoResult<()> {
        try!(self.send_nick("FTButt"));
        try!(self.send_user());
        loop {
            let line = try!(self.read_line());
            let line = String::from_utf8_lossy(line.as_slice());
            let (source, params, msg) = Bot::parse_line(line.as_slice());
            let params = params.as_slice();
            let command = match params.get(0) {
                Some(command) => command,
                None => continue,
            };
            let params = params.slice_from(1);
            match command {
                &"372" => {
                    try!(self.term.fg(5));
                    println!("{}", msg.unwrap_or(""));
                }
                &"375" => (),
                &"376" => try!(self.send_join("#FTB-Wiki")),
                &"433" => try!(self.send_nick("FTButtocks")),
                &"NOTICE" => {
                    match params.get(0) {
                        Some(&"*") => {
                            try!(self.term.fg(1));
                            print!("NOTICE: ");
                            flush();
                            try!(self.term.fg(3));
                            println!("{}", msg.unwrap_or(""));
                        },
                        Some(chan) => {
                            try!(self.term.fg(9));
                            print!("{} {} NOTICE: ", chan, Bot::shorten(source.unwrap_or("")));
                            flush();
                            try!(self.term.fg(11));
                            println!("{}", msg.unwrap_or(""));
                        },
                        None => println!("HALP: {}", line),
                    }
                }
                &"PRIVMSG" => {
                    try!(self.term.fg(9));
                    print!("{} {}: ", params.get(0).unwrap_or(&""), Bot::shorten(source.unwrap_or("")));
                    flush();
                    try!(self.term.fg(11));
                    println!("{}", msg.unwrap_or(""));
                },
                &"PING" => try!(self.send_pong(msg)),
                _ => {
                    try!(self.term.fg(6));
                    println!("{}, {}, {}, {}", source, command, params, msg);
                },
            }
        }
    }
    fn send(
        &mut self, source: Option<&str>, command: &str, params: &[&str], msg: Option<&str>
    ) -> IoResult<()> {
        match source {
            Some(source) => try!(write!(self.cout, ":{} ", source)),
            None => (),
        }
        try!(write!(self.cout, "{} ", command));
        for param in params.iter() {
            try!(write!(self.cout, "{} ", param));
        }
        match msg {
            Some(msg) => try!(write!(self.cout, ":{}", msg)),
            None => (),
        }
        try!(write!(self.cout, "\r\n"));
        self.cout.flush()
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
        let mut bot = Bot::new();
        match bot.run() {
            Ok(_) => println!("Bot ended ok?"),
            Err(e) => println!("Bot failed: {}", e),
        }
        sleep(Duration::seconds(10));
    }
}
