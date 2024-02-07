use std::{
    fs,
    io::Read,
    path::PathBuf,
    process::{self, Command, Stdio},
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

use genanki_rs::{Deck, Field, Model, Note, Package, Template};
use regex::Regex;
use relm4::{ComponentSender, Worker};
use srtlib::Subtitles;

use crate::{converter, AppInMsg, AudioExt};

pub struct AsyncHandler;

#[derive(Debug)]
pub enum AsyncHandlerInMsg {
    GenImage(PathBuf, String),
    GenDeck(String, PathBuf),
    ConvertMP3(PathBuf),
    SplitAudio(converter::MyArgs, PathBuf),
}

impl AsyncHandler {
    fn create_command() -> Command {
        if cfg!(unix) {
            Command::new("ffmpeg")
        } else if cfg!(windows) {
            Command::new("ffmpeg.exe")
        } else {
            panic!("Unsupported OS possibly.")
        }
    }

    fn update_buffer(contents: &str, clear: bool, sender: &ComponentSender<Self>) {
        sender
            .output(AppInMsg::UpdateBuffer(contents.to_string(), clear))
            .unwrap();
    }

    fn gen_image(&self, path: PathBuf, prefix: &str, sender: &ComponentSender<Self>) {
        let mut command = AsyncHandler::create_command();
        command.args([
            "-y",
            "-i",
            &path.as_os_str().to_str().unwrap_or(""),
            "-an",
            "-vcodec",
            "copy",
            &format!("{}.jpg", prefix),
        ]);
        AsyncHandler::update_buffer("Creating cover file...", true, sender);
        let child = command.output().unwrap();
        AsyncHandler::update_buffer("Done!\n", false, sender);
    }

    fn convert_mp3(
        &self,
        audio_path: PathBuf,
        // audio_ext: Option<crate::AudioExt>,
        sender: &ComponentSender<AsyncHandler>,
    ) {
        let regex = Regex::new(r"size=.* time=(.*?) .* speed=(.*x)").unwrap();
        let (tx, rx): (Sender<String>, Receiver<String>) = mpsc::channel();
        let thread_tx = tx.clone();
        let audio_ext = audio_path.extension().unwrap_or_default();
        //TODO if can be removed probably
        if audio_ext == "m4b" {
            let mut converted_path = audio_path.clone();
            converted_path.set_extension("mp3");
            AsyncHandler::update_buffer(
                "Converting to mp3, this'll take a few minutes...",
                false,
                sender,
            );
            let mut command = AsyncHandler::create_command();
            command.stdout(Stdio::piped()).stderr(Stdio::piped()).args([
                "-stats",
                "-v",
                "quiet",
                "-n", //TODO reeeeeeeeeeemove someday
                // "-y",
                "-i",
                audio_path.as_os_str().to_str().unwrap_or(""),
                "-vn",
                "-acodec",
                "libmp3lame",
                &converted_path.as_os_str().to_str().unwrap_or(""),
            ]);
            let mut child = command.spawn().unwrap();
            let mut stderr = child.stderr.take().unwrap();

            thread::spawn(move || loop {
                let mut buf = [0; 80];
                match stderr.read(&mut buf) {
                    Err(err) => {
                        println!("{}] Error reading from stream: {}", line!(), err);
                        break;
                    }
                    Ok(got) => {
                        if got == 0 {
                            tx.send(String::from("STOP")).unwrap();
                            break;
                        } else {
                            let str = String::from_utf8_lossy(&buf);
                            let str = regex.replace_all(&str, "Converting... $1 - $2");
                            let str = str.trim_end_matches('\0');
                            let str = str.trim_end_matches('\r');
                            tx.send(str.to_string()).unwrap();
                        }
                    }
                }
            });

            // let sender2 = sender.clone();
            loop {
                if let Ok(msg) = rx.recv() {
                    if msg == "STOP" {
                        AsyncHandler::update_buffer("Converting Done!", false, sender);
                        break;
                    } else {
                        AsyncHandler::update_buffer(&msg, true, sender);
                    }
                }
            }
        }
    }

    fn split_audio(
        &self,
        mut args: converter::MyArgs,
        path: PathBuf,
        sender: &ComponentSender<AsyncHandler>,
    ) {
        let audio_ext = path.extension().unwrap_or_default();
        // let path =
        if audio_ext == "m4b" {
            let mut converted_path = path.clone();
            converted_path.set_extension("mp3");
            args.audiobook = converted_path;
            // converted_path
        }
        // } else {
        //     path
        // };

        let (tx, rx): (Sender<String>, Receiver<String>) = mpsc::channel();
        let thread_tx = tx.clone();
        thread::spawn(move || {
            converter::process(args, thread_tx.clone());
            thread_tx.send(String::from("STOP")).unwrap();
        });
        loop {
            if let Ok(msg) = rx.recv() {
                if msg == "STOP" {
                    if audio_ext == "m4b" {
                        let mut converted_path = path.clone();
                        converted_path.set_extension("mp3");
                        let _ = fs::remove_file(&converted_path);
                    }
                    AsyncHandler::update_buffer("Extracting done!", false, sender);
                    break;
                } else {
                    AsyncHandler::update_buffer(&msg, true, sender);
                }
            }
        }
    }

    fn gen_deck(&self, prefix: &str, srt_path: PathBuf, sender: &ComponentSender<AsyncHandler>) {
        AsyncHandler::update_buffer("Converting to apkg...", false, sender);
        let model = Model::new(
            170655988708,
            "audiobook2srs",
            vec![
                Field::new("Audio"),
                Field::new("Image"),
                Field::new("Sentence"),
            ],
            vec![Template::new("Card 1")
                .qfmt("{{Sentence}}")
                .afmt(r#"{{FrontSide}}<hr id="answer">{{Audio}} {Image}"#)],
        );
        let now = SystemTime::now();
        let timestamp = now.duration_since(UNIX_EPOCH).unwrap().as_millis();
        let mut deck = Deck::new(
            timestamp as i64,
            prefix,
            &format!(
                "{} - Generated by https://github.com/asayake-b5/audiobook2srs",
                prefix
            ),
        );

        let subs = Subtitles::parse_from_file(srt_path, Some("utf8"))
            .unwrap()
            .to_vec();

        let mut files: Vec<String> = Vec::with_capacity(subs.len() + 100);

        // subs.sort();

        for sub in subs {
            files.push(format!("./gen/{}/{}-{}.mp3", prefix, prefix, sub.num - 1));
            deck.add_note(
                Note::new(
                    model.clone(),
                    vec![
                        &format!("[sound:{}-{}.mp3]", prefix, sub.num - 1),
                        &format!("<img src=\"{}.jpg\">", prefix),
                        &sub.text,
                    ],
                )
                .unwrap(),
            );
        }

        let mut files2: Vec<&str> = files.iter().map(|s| &**s).collect();
        let cover = format!("{}.jpg", prefix);
        files2.push(&cover);

        let mut package = Package::new(vec![deck], files2).unwrap();
        package.write_to_file(&format!("{}.apkg", prefix)).unwrap();
        AsyncHandler::update_buffer("Conversion to apkg done!!\n", true, sender);
        AsyncHandler::update_buffer("Cleaning up..", false, sender);
        let _ = fs::remove_dir_all(format!("./gen/{}", prefix));
        let _ = fs::remove_file(&cover);
        AsyncHandler::update_buffer("..Done!", false, sender);
    }
}

impl Worker for AsyncHandler {
    type Init = ();
    type Input = AsyncHandlerInMsg;
    type Output = AppInMsg;

    fn init(_init: Self::Init, _sender: ComponentSender<Self>) -> Self {
        Self
    }

    fn update(&mut self, msg: AsyncHandlerInMsg, sender: ComponentSender<Self>) {
        match msg {
            AsyncHandlerInMsg::GenImage(path, prefix) => {
                self.gen_image(path, &prefix, &sender);
                sender.output(AppInMsg::StartConversion).unwrap();
            }
            AsyncHandlerInMsg::GenDeck(prefix, path) => self.gen_deck(&prefix, path, &sender),
            AsyncHandlerInMsg::SplitAudio(args, path) => {
                self.split_audio(args, path, &sender);
                sender.output(AppInMsg::StartGenDeck).unwrap();
            }

            AsyncHandlerInMsg::ConvertMP3(audio_path) => {
                self.convert_mp3(audio_path, &sender);
                sender.output(AppInMsg::StartAudioSplit).unwrap();
            }
        }

        // // Send the result of the calculation back
        // match msg {
        //     AsyncHandlerMsg::DelayedIncrement => sender.output(AppMsg::Increment),
        //     AsyncHandlerMsg::DelayedDecrement => sender.output(AppMsg::Decrement),
        // }
        // .unwrap()
    }
}
