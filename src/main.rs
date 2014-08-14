#![feature(phase)]
#[phase(plugin)]
extern crate regex_macros;
extern crate regex;

use std::io::net::tcp::TcpStream;
use std::io::timer::sleep;
use std::io::{BufferedReader, BufReader, BufferedWriter, EndOfFile, IoError, IoResult};
use std::iter::Repeat;
use std::result::collect;

struct Bot {
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
            cout: BufferedWriter::new(tcp.clone()),
            cin: BufferedReader::new(tcp),
        }
    }
    fn run(&mut self) -> IoResult<()> {
        try!(self.send_nick());
        try!(self.send_user());
        let reg_source = regex!(r"^:(\S)");
        loop {
            let line: IoResult<Vec<u8>> = collect(self.cin.bytes()
                .chain(Repeat::<IoResult<u8>>::new(Err(IoError {
                    kind: EndOfFile,
                    desc: "Connection terminated",
                    detail: None,
                })))
                .take_while(|c| {
                    match c {
                        &Ok(b'\r') => false,
                        &Ok(b'\n') => false,
                        _ => true,
                    }
                }));
            let line = try!(line);
            if line.is_empty() { continue }
            let line = String::from_utf8_lossy(line.as_slice());
            let line = line.as_slice();
            println!("{}", line);
        }
        Ok(())
    }
    fn send(
        &mut self, source: Option<&str>, command: &str, args: &[&str], message: Option<&str>
    ) -> IoResult<()> {
        match source {
            Some(source) => try!(write!(self.cout, ":{} ", source)),
            None => (),
        }
        try!(write!(self.cout, "{} ", command));
        for arg in args.iter() {
            try!(write!(self.cout, "{} ", arg));
        }
        match message {
            Some(message) => try!(write!(self.cout, ":{} ", message)),
            None => (),
        }
        try!(write!(self.cout, "\r\n"));
        self.cout.flush()
    }
    fn send_nick(&mut self) -> IoResult<()> {
        self.send(None, "NICK", ["FTButt"], None)
    }
    fn send_user(&mut self) -> IoResult<()> {
        self.send(None, "USER", ["FTButt", "0", "*"], Some("FTButt"))
    }
}

fn main() {
    loop {
        let mut bot = Bot::new();
        match bot.run() {
            Ok(_) => println!("Bot ended ok?"),
            Err(e) => println!("Bot failed: {}", e),
        }
        sleep(5000);
    }
}
