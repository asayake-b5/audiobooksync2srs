use encoding_rs_io::DecodeReaderBytes;
use futures_util::StreamExt;
use genanki_rs::{Deck, Field, Model, Note, Package, Template};
use regex::Regex;
use srtlib::Subtitles;
use std::{
    ffi::CString,
    future::ready,
    io::{BufRead, BufReader, Read, Stdout},
    net::TcpListener,
    path::PathBuf,
    process::{Command, Stdio},
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

use converter::MyArgs;
use ffmpeg_cli::{FfmpegBuilder, Parameter};
use relm4::{
    gtk::{
        self,
        prelude::{
            BoxExt, ButtonExt, CheckButtonExt, EditableExt, EntryBufferExtManual, EntryExt,
            GtkWindowExt, OrientableExt, TextBufferExt, TextViewExt, WidgetExt,
        },
        Adjustment, EntryBuffer, FileFilter,
    },
    Component, ComponentController, ComponentParts, ComponentSender, Controller, RelmApp,
    RelmWidgetExt, SimpleComponent,
};
use relm4_components::{
    open_button::{OpenButton, OpenButtonSettings},
    open_dialog::OpenDialogSettings,
};

mod converter;

#[derive(Debug, Eq, PartialEq)]
enum ImageMode {
    Extract,
    None,
    Custom,
}

#[derive(Debug)]
struct AppModel {
    open_srt: Controller<OpenButton>,
    srt_path: PathBuf,
    open_audio: Controller<OpenButton>,
    audio_path: PathBuf,
    audio_ext: Option<AudioExt>,
    prefix: EntryBuffer,
    image: ImageMode,
    buffer: gtk::TextBuffer,
    offset_before: f64,
    offset_after: f64,
    show_button: bool,
}

#[derive(Debug, PartialEq, Eq)]
enum AudioExt {
    M4b,
    Mp3,
}

#[derive(Debug)]
enum DialogOrigin {
    Audio,
    Srt,
}

#[derive(Debug)]
enum OffsetDirection {
    Before,
    After,
}

#[derive(Debug)]
enum AppInMsg {
    UpdateBuffer(String),
    SetImageMode(ImageMode),
    Recheck,
    UpdateOffset(OffsetDirection, f64),
    Start,
    Open(PathBuf, DialogOrigin),
}

#[derive(Debug)]
enum AppOutMsg {
    Scroll,
}

#[relm4::component]
impl SimpleComponent for AppModel {
    type Input = AppInMsg;

    type Output = AppOutMsg;
    type Init = u8;

    // Initialize the UI.
    fn init(
        _: Self::Init,
        root: &Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let srt_filter = FileFilter::new();
        srt_filter.add_pattern("*.srt");
        srt_filter.set_name(Some("Subtitle files (.srt)"));

        let open_srt = OpenButton::builder()
            .launch(OpenButtonSettings {
                dialog_settings: OpenDialogSettings {
                    folder_mode: false,
                    cancel_label: String::from("Cancel"),
                    accept_label: String::from("Select"),
                    create_folders: true,
                    is_modal: true,
                    // filter:
                    filters: vec![srt_filter],
                },
                text: "Open file",
                recently_opened_files: None,
                max_recent_files: 0,
            })
            .forward(sender.input_sender(), |path| {
                AppInMsg::Open(path, DialogOrigin::Srt)
            });

        let audio_filter = FileFilter::new();
        audio_filter.add_pattern("*.mp3");
        audio_filter.add_pattern("*.m4b");
        audio_filter.add_pattern("*.m4a");
        audio_filter.set_name(Some("Audio files (.mp3, .m4b, .m4a)"));

        let open_audio = OpenButton::builder()
            .launch(OpenButtonSettings {
                dialog_settings: OpenDialogSettings {
                    folder_mode: false,
                    cancel_label: String::from("Cancel"),
                    accept_label: String::from("Select"),
                    create_folders: true,
                    is_modal: true,
                    // filter:
                    filters: vec![audio_filter],
                },
                text: "Open file",
                recently_opened_files: None,
                max_recent_files: 0,
            })
            .forward(sender.input_sender(), |path| {
                AppInMsg::Open(path, DialogOrigin::Audio)
            });

        let model = AppModel {
            prefix: EntryBuffer::new(Some("MyAudiobook")),
            open_srt,
            open_audio,
            buffer: gtk::TextBuffer::new(None),
            image: ImageMode::None,
            audio_ext: None,
            srt_path: PathBuf::from(""),
            audio_path: PathBuf::from(""),
            show_button: false,
            offset_before: 0.0,
            offset_after: 0.0,
        };

        let widgets = view_output!();

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>) {
        match msg {
            AppInMsg::UpdateBuffer(msg) => {
                let (mut start, mut end) = self.buffer.bounds();
                self.buffer.delete(&mut start, &mut end);
                self.buffer.insert_at_cursor(&msg);
            }
            AppInMsg::SetImageMode(mode) => {
                self.image = mode;
            }
            AppInMsg::Start => {
                let mut args = MyArgs {
                    prefix: self.prefix.text().to_string().replace(' ', "_"),
                    audiobook: self.audio_path.clone(),
                    subtitle: self.srt_path.clone(),
                    start_offset: self.offset_before as i32,
                    end_offset: self.offset_after as i32,
                };

                // let regex = Regex::new(r"size=.* time=(.*?) .* speed=(.*x)").unwrap();

                // if self.image == ImageMode::Extract {
                //     let mut command = if cfg!(unix) {
                //         Command::new("ffmpeg")
                //     } else if cfg!(windows) {
                //         Command::new("ffmpeg.exe")
                //     } else {
                //         panic!("Unsupported OS possibly.")
                //     };
                //     command.args([
                //         "-y",
                //         "-i",
                //         &self.audio_path.as_os_str().to_str().unwrap_or(""),
                //         "-an",
                //         "-vcodec",
                //         "copy",
                //         "cover.jpg",
                //     ]);
                //     self.buffer.insert_at_cursor("Creating cover file..");
                //     let child = command.output().unwrap();
                //     self.buffer.insert_at_cursor("Done!");
                //     // sender.output(AppOutMsg::Scroll).unwrap();
                // }
                // let (tx, rx): (Sender<String>, Receiver<String>) = mpsc::channel();
                // let thread_tx = tx.clone();

                // if self.audio_ext == Some(AudioExt::M4b) {
                //     let mut converted_path = self.audio_path.clone();
                //     converted_path.set_extension("mp3");
                //     self.buffer
                //         .insert_at_cursor("Converting to mp3, this'll take a few minutes..");
                //     let mut command = if cfg!(unix) {
                //         Command::new("ffmpeg")
                //     } else if cfg!(windows) {
                //         Command::new("ffmpeg.exe")
                //     } else {
                //         panic!("Unsupported OS possibly.")
                //     };
                //     command.stdout(Stdio::piped()).stderr(Stdio::piped()).args([
                //         "-stats",
                //         "-v",
                //         "quiet",
                //         "-n", //TODO reeeeeeeeeeemove someday
                //         "-y",
                //         "-i",
                //         &self.audio_path.as_os_str().to_str().unwrap_or(""),
                //         "-vn",
                //         "-acodec",
                //         "libmp3lame",
                //         &converted_path.as_os_str().to_str().unwrap_or(""),
                //     ]);
                //     let mut child = command.spawn().unwrap();
                //     let mut stderr = child.stderr.take().unwrap();

                //     let (tx, rx): (Sender<String>, Receiver<String>) = mpsc::channel();
                //     thread::spawn(move || loop {
                //         let mut buf = [0; 80];
                //         match stderr.read(&mut buf) {
                //             Err(err) => {
                //                 println!("{}] Error reading from stream: {}", line!(), err);
                //                 break;
                //             }
                //             Ok(got) => {
                //                 if got == 0 {
                //                     tx.send(String::from("STOP")).unwrap();
                //                     break;
                //                 } else {
                //                     let str = String::from_utf8_lossy(&buf);
                //                     let str = regex.replace_all(&str, "Converting... $1 - $2");
                //                     let str = str.trim_end_matches('\0');
                //                     let str = str.trim_end_matches('\r');
                //                     tx.send(str.to_string()).unwrap();
                //                 }
                //             }
                //         }
                //     });

                //     let sender2 = sender.clone();
                //     thread::spawn(move || loop {
                //         if let Ok(msg) = rx.recv() {
                //             if msg == "STOP" {
                //                 sender2.input(AppInMsg::UpdateBuffer(String::from(
                //                     "Converting Done!",
                //                 )));
                //                 break;
                //             } else {
                //                 sender2.input(AppInMsg::UpdateBuffer(msg));
                //             }
                //         }
                //     });
                // }
                // //TODO await here, otherewise spawn goes before end of convert

                // if self.audio_ext == Some(AudioExt::M4b) {
                //     let mut converted_path = self.audio_path.clone();
                //     converted_path.set_extension("mp3");
                //     args.audiobook = converted_path;
                // };
                // thread::spawn(move || {
                //     converter::process(args, thread_tx.clone());
                //     thread_tx.send(String::from("STOP")).unwrap();
                // });
                // let sender2 = sender.clone();
                // //TODO PROPER ASYNC
                // let mut wait = true;
                // thread::spawn(move || loop {
                //     if let Ok(msg) = rx.recv() {
                //         if msg == "STOP" {
                //             sender2.input(AppInMsg::UpdateBuffer(String::from(
                //                 "Extracting audio files done!",
                //             )));
                //             wait = false;
                //             break;
                //         } else {
                //             // self.buffer.insert_at_cursor(&msg);
                //             sender2.input(AppInMsg::UpdateBuffer(msg));
                //         }
                //     }
                // });

                //TODO handle custom cover file

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
                    &self.prefix.text(),
                    &format!(
                        "{} - Generated by https://github.com/asayake-b5/audiobook2srs",
                        &self.prefix.text()
                    ),
                );

                let subs = Subtitles::parse_from_file(&self.srt_path, Some("utf8"))
                    .unwrap()
                    .to_vec();

                let mut files: Vec<String> = Vec::with_capacity(subs.len() + 100);

                // subs.sort();

                for sub in subs {
                    files.push(format!(
                        "./gen/{}/{}-{}.mp3",
                        &self.prefix, &self.prefix, sub.num
                    ));
                    deck.add_note(
                        Note::new(
                            model.clone(),
                            vec![
                                &format!("[sound:{}-{}.mp3]", self.prefix, sub.num),
                                "",
                                &sub.text,
                            ],
                        )
                        .unwrap(),
                    );
                }

                let files2: Vec<&str> = files.iter().map(|s| &**s).collect();
                let mut package = Package::new(vec![deck], files2).unwrap();
                package
                    .write_to_file(&format!("{}.apkg", self.prefix))
                    .unwrap();
                sender.input(AppInMsg::UpdateBuffer(String::from("Done!")));
            }
            AppInMsg::UpdateOffset(dir, val) => match dir {
                OffsetDirection::Before => {
                    self.offset_before = val;
                }
                OffsetDirection::After => {
                    self.offset_after = val;
                }
            },
            AppInMsg::Recheck => {
                self.show_button = self.prefix.length() > 0
                    && !self.audio_path.as_os_str().is_empty()
                    && !self.srt_path.as_os_str().is_empty();
            }
            AppInMsg::Open(path, origin) => {
                match origin {
                    DialogOrigin::Audio => {
                        if path.extension().unwrap() == "m4b" {
                            self.audio_ext = Some(AudioExt::M4b);
                        } else {
                            self.audio_ext = Some(AudioExt::Mp3);
                        }
                        self.audio_path = path
                    }
                    DialogOrigin::Srt => self.srt_path = path,
                };
                self.show_button = self.prefix.length() > 0
                    && !self.audio_path.as_os_str().is_empty()
                    && !self.srt_path.as_os_str().is_empty();
            }
        }
    }

    view! {
        gtk::Window {
            set_title: Some("Audiobook to Ren'py"),
            set_default_width: 600,
            set_default_height: 400,

            gtk::Box {
                set_orientation: gtk::Orientation::Vertical,
                set_spacing: 5,
                set_margin_all: 5,

                gtk::Box {
                    set_spacing: 5,
                    set_margin_all: 5,
                    set_orientation: gtk::Orientation::Horizontal,
                    gtk::Label {
                        set_label: "Prefix for the audio files (please use something somewhat unique)"

                    },
                    gtk::Entry {
                        set_buffer: &model.prefix,
                        connect_changed => AppInMsg::Recheck,

                    },
                },


                gtk::Box {
                    set_spacing: 5,
                    set_margin_all: 5,
                    set_orientation: gtk::Orientation::Horizontal,
                    gtk::Label {
                        set_label: "Path to the .srt file"

                    },
                    append = model.open_srt.widget(),
                    gtk::Label {
                        #[watch]
                        set_label: &model.srt_path.to_string_lossy()
                    }
                },
                gtk::Box {
                    set_spacing: 5,
                    set_margin_all: 5,
                    set_orientation: gtk::Orientation::Horizontal,
                    gtk::Label {
                        set_label: "Path to the audio file"
                    },
                    append = model.open_audio.widget(),
                    gtk::Label {
                        #[watch]
                        set_label: &model.audio_path.to_string_lossy()
                    }
                },

                // gtk::Box {
                //     set_spacing: 5,
                //     set_margin_all: 5,
                //     set_orientation: gtk::Orientation::Horizontal,
                //     gtk::Label {
                //         set_label: "Path to the epub file (optional, for furigana),"
                //     },
                //     append = model.open_epub.widget(),
                //     gtk::Label {
                //         #[watch]
                //         set_label: &model.epub_path.to_string_lossy()
                //     }
                // },

                gtk::Box {
                    set_spacing: 5,
                    set_margin_all: 5,
                    set_orientation: gtk::Orientation::Horizontal,
                    gtk::Label {
                        set_label: "Offsets"
                    },
                gtk::Box {
                    set_orientation: gtk::Orientation::Vertical,
                    relm4::gtk::SpinButton::builder()
                    .adjustment(&Adjustment::new(0.0, -500.0, 500.0, 1.0, 0.0, 0.0))
                    .build(){
                        connect_value_changed[sender] => move |x| {
                            sender.input(AppInMsg::UpdateOffset(OffsetDirection::Before, x.value()))
                    }},
                    gtk::Label {
                            set_label: "Before (ms)"
                        }
                },
                gtk::Box {
                    set_orientation: gtk::Orientation::Vertical,
                    relm4::gtk::SpinButton::builder()
                    .adjustment(&Adjustment::new(0.0, -500.0, 500.0, 1.0, 0.0, 0.0))
                    .build(){
                        connect_value_changed[sender] => move |x| {
                            sender.input(AppInMsg::UpdateOffset(OffsetDirection::After, x.value()))
                        }
                    },
                        gtk::Label {
                            set_label: "After (ms)"
                        }
                    },
                },

                gtk::Box {
                    set_spacing: 5,
                    set_margin_all: 5,
                    set_orientation: gtk::Orientation::Horizontal,
                    gtk::Label {
                        set_label: "Number of threads (TODO)"
                    },
                    relm4::gtk::SpinButton::builder()
                    .adjustment(&Adjustment::new(4.0, 0.0, 12.0, 1.0, 0.0, 0.0))
                    .build(){
                        connect_value_changed[sender] => move |x| {
                            sender.input(AppInMsg::UpdateOffset(OffsetDirection::Before, x.value()))
                    }},
                },

                gtk::Box {
                    set_spacing: 5,
                    set_margin_all: 5,
                    set_orientation: gtk::Orientation::Horizontal,
                    gtk::Label {
                        set_label: "Image:"
                    },
                    append: group = &gtk::CheckButton {
                        set_label: Some("None"),
                        set_active: true,
                        connect_toggled[sender] => move |btn| {
                        if btn.is_active() {
                            sender.input(AppInMsg::SetImageMode(ImageMode::None));
                        }
                    }
                    },
                    //TODO if file ext = m4b
                    append = &gtk::CheckButton {
                        set_label: Some("Extract from m4b"),
                        set_active: false,
                        set_group: Some(&group),
                        connect_toggled[sender] => move |btn| {
                        if btn.is_active() {
                            sender.input(AppInMsg::SetImageMode(ImageMode::Extract));
                        }
                    }
                    },
                    append = &gtk::CheckButton {
                        set_label: Some("From file"),
                        set_group: Some(&group),
                        set_active: false,
                        connect_toggled[sender] => move |btn| {
                        if btn.is_active() {
                            sender.input(AppInMsg::SetImageMode(ImageMode::Custom));
                        }
                    }
                    },

                },


                append = if model.show_button {
                    gtk::Button::with_label("Generate Deck !") {
                        connect_clicked[sender] => move |_| {
                            sender.input(AppInMsg::Start);
                        }
                }} else {
                    gtk::Label{
                        set_label: "Please fill all mandatory fields"
                    }
                },


                gtk::Box {
                    set_orientation: gtk::Orientation::Vertical,
                    set_margin_all: 5,

                    gtk::ScrolledWindow {
                        set_min_content_height: 380,

                        #[wrap(Some)]
                        set_child = &gtk::TextView {
                            set_buffer: Some(&model.buffer),
                            set_editable: false,
                            // #[watch]
                            // set_visible: model.file_name.is_some(),
                        },
                    }},
                // else if model.show_indicator {
                //     gtk::Spinner {
                //         set_spinning: true,
                //     }
                // }

            }
        }
    }
}

// #[tokio::main]
fn main() {
    // rayon::ThreadPoolBuilder::new()
    //     .num_threads(4)
    //     .build_global()
    //     .unwrap();
    let app = RelmApp::new("relm4.test.simple");
    app.run::<AppModel>(0);
}

// let mut command = if cfg!(unix) {
//     Command::new("ffmpeg")
// } else if cfg!(windows) {
//     Command::new("ffmpeg.exe")
// } else {
//     panic!("Unsupported OS possibly.")
// };
// command.stdout(Stdio::piped()).stderr(Stdio::piped()).args([
//     // "-progress",
//     // &prog_url,
//     "-y",
//     "-i",
//     "adachi3.mp3",
//     "-vn",
//     "-acodec",
//     // "libmp3lame",
//     "copy",
//     "output.mp3",
// ]);
// let mut child = command.spawn().unwrap();
// let mut stderr = child.stderr.take().unwrap();
