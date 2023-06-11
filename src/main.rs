use mediawiki::{Error as MwError, Mediawiki};
use reqwest::blocking::Client;
use serde_json::{json, to_string_pretty, Map as JsonMap, Value as Json};
use std::{
    cmp::max,
    fmt::{self, Display, Write as _},
    fs::{read_to_string, rename, File, OpenOptions},
    io::{Error as IoError, Read, Write},
    num::ParseIntError,
    sync::mpsc::{channel, Receiver, Sender},
    thread::{sleep, spawn},
    time::Duration,
};
use url::form_urlencoded::Serializer;

#[derive(Debug)]
enum Error {
    Mediawiki(MwError),
    Io(IoError),
    ParseInt(ParseIntError),
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
    spawn(move || mw_thread(send));
    webhook_thread(recv);
}
struct Title<'a>(&'a str);
impl<'a> Display for Title<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "**{}**", self.0)
    }
}
struct Change {
    title: String,
    user: String,
    action: String,
    description: String,
    timestamp: String,
    link: Option<String>,
    diff: Option<i64>,
    comment: Option<String>,
    extra: Vec<(String, String)>,
}
impl Change {
    #[allow(dead_code)]
    fn make_embed(self) -> Json {
        let mut embed = JsonMap::new();
        embed.insert("type".into(), Json::String("rich".into()));
        embed.insert("title".into(), Json::String(self.title));
        embed.insert("timestamp".into(), Json::String(self.timestamp));
        embed.insert("author".into(), json!({"name": self.action}));
        if let Some(link) = self.link {
            embed.insert("url".into(), Json::String(link));
        }
        if let Some(comment) = self.comment {
            embed.insert("description".into(), Json::String(comment));
        }
        let mut fields = Vec::new();
        if let Some(diff) = self.diff {
            fields.push(json!({"name": "Diff", "inline": true, "value": format!("{diff:+}")}));
        }
        for (key, value) in self.extra {
            fields.push(json!({"name": key, "inline": true, "value": value}));
        }
        embed.insert("fields".into(), Json::Array(fields));
        json!({"embeds": [Json::Object(embed)], "username": self.user})
    }
    fn make_message(self) -> Json {
        let mut message = String::new();
        if let Some(diff) = self.diff {
            write!(&mut message, "[{diff:+}] ").unwrap();
        }
        write!(&mut message, "{}", self.description).unwrap();
        if let Some(link) = self.link {
            write!(&mut message, " <{link}>").unwrap();
        }
        if let Some(comment) = self.comment {
            write!(&mut message, "\n```\n{comment}\n```").unwrap();
        }
        json!({"content": message, "username": self.user})
    }
}
fn make_article_link(title: &str) -> String {
    let args = &[("title", title)];
    let query = Serializer::new(String::new()).extend_pairs(args).finish();
    format!("https://ftb.fandom.com/?{query}")
}
fn make_revision_link(_title: &str, oldid: &str) -> String {
    let args = &[("oldid", oldid)];
    let query = Serializer::new(String::new()).extend_pairs(args).finish();
    format!("https://ftb.fandom.com/?{query}")
}
fn make_diff_link(_title: &str, diff: &str) -> String {
    let args = &[("diff", diff)];
    let query = Serializer::new(String::new()).extend_pairs(args).finish();
    format!("https://ftb.fandom.com/?{query}")
}
enum ChangeError {
    Unhandled,
    Ignored,
}
fn process_change(change: &Json) -> Result<Change, ChangeError> {
    let kind = change["type"].as_str().ok_or(ChangeError::Unhandled)?;
    let user = change["user"]
        .as_str()
        .ok_or(ChangeError::Unhandled)?
        .to_owned();
    let title = change["title"]
        .as_str()
        .ok_or(ChangeError::Unhandled)?
        .to_owned();
    if title.starts_with("Translations:") {
        return Err(ChangeError::Ignored);
    }
    let timestamp = change["timestamp"]
        .as_str()
        .ok_or(ChangeError::Unhandled)?
        .to_owned();
    let comment = change["comment"]
        .as_str()
        .filter(|x| !x.is_empty())
        .map(|x| x.to_owned());
    let oldlen = change["oldlen"].as_i64().ok_or(ChangeError::Unhandled)?;
    let newlen = change["newlen"].as_i64().ok_or(ChangeError::Unhandled)?;
    let revid = change["revid"].as_i64().ok_or(ChangeError::Unhandled)?;
    let logaction = change["logaction"].as_str().ok_or(ChangeError::Unhandled);
    let logtype = change["logtype"].as_str().ok_or(ChangeError::Unhandled);
    let logparams = &change["logparams"];
    let diff = if oldlen != 0 && newlen != 0 {
        Some(newlen - oldlen)
    } else {
        None
    };
    let ftitle = Title(&title);
    let (action, description, link, extra) = match kind {
        "categorize" => return Err(ChangeError::Ignored),
        "edit" => (
            "Edit".into(),
            format!("Edited {ftitle}"),
            Some(make_diff_link(&title, &revid.to_string())),
            Vec::new(),
        ),
        "new" => (
            "New".into(),
            format!("Created {ftitle}"),
            Some(make_revision_link(&title, &revid.to_string())),
            Vec::new(),
        ),
        "log" => match (logtype?, logaction?) {
            ("curseprofile", "comment-created") => (
                "Profile comment".into(),
                format!("Commented on profile for {ftitle}"),
                Some(make_article_link(&title)),
                Vec::new(),
            ),
            ("curseprofile", "comment-deleted") => (
                "Profile delete comment".into(),
                format!("Deleted comment on profile for {ftitle}"),
                Some(make_article_link(&title)),
                Vec::new(),
            ),
            ("curseprofile", "comment-replied") => (
                "Profile reply comment".into(),
                format!("Replied to comment on profile for {ftitle}"),
                Some(make_article_link(&title)),
                Vec::new(),
            ),
            ("curseprofile", "profile-edited") => (
                "Profile edit".into(),
                format!(
                    "Edited {} for {}",
                    logparams["4:section"]
                        .as_str()
                        .ok_or(ChangeError::Unhandled)?,
                    ftitle
                ),
                Some(make_article_link(&title)),
                Vec::new(),
            ),
            ("delete", "delete") => (
                "Delete".into(),
                format!("Deleted {ftitle}"),
                Some(make_article_link(&title)),
                Vec::new(),
            ),
            ("move", "move") => (
                "Move".into(),
                format!(
                    "Moved {} to {}",
                    ftitle,
                    Title(
                        logparams["target_title"]
                            .as_str()
                            .ok_or(ChangeError::Unhandled)?
                    )
                ),
                Some(make_article_link(&title)),
                Vec::new(),
            ),
            ("upload", "upload") => (
                "Upload".into(),
                format!("Uploaded {ftitle}"),
                Some(make_article_link(&title)),
                Vec::new(),
            ),
            ("upload", "overwrite") => (
                "Upload".into(),
                format!("Uploaded a new version of {ftitle}"),
                Some(make_article_link(&title)),
                Vec::new(),
            ),
            ("pagetranslation", "mark") => (
                "Page translation".into(),
                format!("Marked {ftitle} for translation"),
                Some(make_article_link(&title)),
                Vec::new(),
            ),
            ("protect", "protect") => (
                "Protection".into(),
                format!(
                    "Modified protection for {} to {}",
                    ftitle,
                    logparams["description"]
                        .as_str()
                        .ok_or(ChangeError::Unhandled)?,
                ),
                Some(make_article_link(&title)),
                Vec::new(),
            ),
            _ => return Err(ChangeError::Unhandled),
        },
        _ => return Err(ChangeError::Unhandled),
    };
    Ok(Change {
        title,
        user,
        action,
        description,
        timestamp,
        link,
        diff,
        comment,
        extra,
    })
}
fn mw_thread(send: Sender<Change>) {
    fn load_latest() -> Result<i64, Error> {
        let mut file = File::open("latest.txt")?;
        let mut s = String::new();
        file.read_to_string(&mut s)?;
        Ok(s.trim().parse()?)
    }
    fn save_latest(n: i64) -> Result<(), Error> {
        let mut file = File::create("next.txt")?;
        write!(&mut file, "{n}")?;
        drop(file);
        rename("next.txt", "latest.txt")?;
        Ok(())
    }
    let mw = Mediawiki::login_path("ftb.json").unwrap();
    let mut latest = load_latest().unwrap_or(0);
    println!("Resuming at {latest}");
    let mut rcfile = OpenOptions::new()
        .write(true)
        .append(true)
        .create(true)
        .open("rc.txt")
        .unwrap();
    let mut todofile = OpenOptions::new()
        .write(true)
        .append(true)
        .create(true)
        .open("todo.txt")
        .unwrap();
    let mut changes = Vec::new();
    loop {
        let previous = latest;
        for change in mw.query_recentchanges(20) {
            match change {
                Ok(change) => {
                    let id = change["rcid"].as_i64().unwrap();
                    latest = max(id, latest);
                    if id <= previous || previous == 0 {
                        break;
                    }
                    let pretty = to_string_pretty(&change).unwrap_or_default();
                    writeln!(&mut rcfile, "{pretty}").unwrap();
                    match process_change(&change) {
                        Ok(change) => changes.push(change),
                        Err(ChangeError::Unhandled) => writeln!(&mut todofile, "{pretty}").unwrap(),
                        Err(ChangeError::Ignored) => (),
                    }
                }
                Err(e) => {
                    println!("Failed to query RC: {e:?}");
                    break;
                }
            }
        }
        if previous != latest {
            save_latest(latest).unwrap();
        }
        for change in changes.drain(..).rev() {
            send.send(change).unwrap();
        }
        sleep(Duration::from_secs(10))
    }
}
fn webhook_thread(recv: Receiver<Change>) -> ! {
    let webhook = read_to_string("webhook.txt").unwrap();
    let webhook = webhook.trim();
    let client = Client::new();
    loop {
        for change in &recv {
            let embed = change.make_message();
            loop {
                let response = match client
                    .post(webhook)
                    .query(&[("wait", "true")])
                    .json(&embed)
                    .send()
                {
                    Ok(response) => response,
                    Err(err) => {
                        println!("Failed to send: {err}");
                        sleep(Duration::from_secs(10));
                        continue;
                    }
                };
                let status = response.status();
                if status.is_success() {
                    break;
                }
                let json: Json = match response.json() {
                    Ok(x) => x,
                    Err(err) => {
                        println!("{status}");
                        println!("{err}");
                        sleep(Duration::from_secs(10));
                        continue;
                    }
                };
                match status.as_u16() {
                    429 => {
                        let retry_after = json["retry_after"].as_u64().unwrap_or(1000);
                        println!("Sleeping for {retry_after}ms");
                        sleep(Duration::from_millis(retry_after));
                    }
                    _ => {
                        println!("{status}");
                        println!("{json}");
                        println!("{embed}");
                        sleep(Duration::from_secs(10));
                    }
                }
            }
        }
    }
}
