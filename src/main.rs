use genanki_rs::{Deck, Field, Model, Note, Package, Template};
use srtlib::Subtitles;
use std::{convert::identity, path::PathBuf};
use worker::{AsyncHandler, AsyncHandlerInMsg};

use converter::MyArgs;
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
    RelmWidgetExt, SimpleComponent, WorkerController,
};
use relm4_components::{
    open_button::{OpenButton, OpenButtonSettings},
    open_dialog::OpenDialogSettings,
};

mod converter;
mod worker;

#[derive(Debug, Eq, PartialEq)]
enum ImageMode {
    Extract,
    None,
    Custom,
}

// #[derive(Debug)]
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
    worker: WorkerController<AsyncHandler>,
    sensitive: bool,
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
pub enum AppInMsg {
    UpdateBuffer(String, bool),
    SetImageMode(ImageMode),
    Recheck,
    UpdateOffset(OffsetDirection, f64),
    Start,
    Open(PathBuf, DialogOrigin),
    StartConversion,
    StartAudioSplit,
    StartGenDeck,
    Ended,
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
            sensitive: true,
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
            worker: AsyncHandler::builder()
                .detach_worker(())
                .forward(sender.input_sender(), identity),
        };

        let widgets = view_output!();

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>) {
        match msg {
            AppInMsg::Ended => {
                self.sensitive = true;
            }
            AppInMsg::UpdateBuffer(msg, delete) => {
                if delete {
                    let (mut start, mut end) = self.buffer.bounds();
                    self.buffer.delete(&mut start, &mut end);
                }
                self.buffer.insert_at_cursor(&msg);
            }
            AppInMsg::SetImageMode(mode) => {
                self.image = mode;
            }
            AppInMsg::StartConversion => {
                self.worker
                    .emit(AsyncHandlerInMsg::ConvertMP3(self.audio_path.clone()));
            }
            AppInMsg::StartAudioSplit => {
                let args = MyArgs {
                    prefix: self.prefix.text().to_string().replace(' ', "_"),
                    audiobook: self.audio_path.clone(),
                    subtitle: self.srt_path.clone(),
                    start_offset: self.offset_before as i32,
                    end_offset: self.offset_after as i32,
                };
                self.worker
                    .emit(AsyncHandlerInMsg::SplitAudio(args, self.audio_path.clone()))
            }

            AppInMsg::StartGenDeck => self.worker.emit(AsyncHandlerInMsg::GenDeck(
                self.prefix.text().to_string().replace(' ', "_"),
                self.srt_path.clone(),
                self.image == ImageMode::Extract
            )),

            AppInMsg::Start => {
                self.sensitive = false;
                match self.image {
                    ImageMode::Extract => {
                        self.worker.emit(AsyncHandlerInMsg::GenImage(
                            self.audio_path.clone(),
                            self.prefix.text().to_string(),
                        ));
                    }
                    _ => {
                        sender.input(AppInMsg::StartConversion);
                    } // ImageMode::None => todo!(),
                    // ImageMode::Custom => todo!(),
                };

                //TODO handle custom cover file
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
            set_title: Some("Audiobook to Anki"),
            set_default_width: 600,
            set_default_height: 400,

            gtk::Box {
                set_orientation: gtk::Orientation::Vertical,
                set_spacing: 5,
                set_margin_all: 5,

                gtk::Box {
                    #[watch]
                    set_sensitive: model.sensitive,

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
                    #[watch]
                    set_sensitive: model.sensitive,
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
                    #[watch]
                    set_sensitive: model.sensitive,

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

                gtk::Box {
                    #[watch]
                    set_sensitive: model.sensitive,
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
                },

                gtk::Box {
                    set_spacing: 5,
                    set_margin_all: 5,
                    set_orientation: gtk::Orientation::Horizontal,
                        #[watch]
                        set_sensitive: model.sensitive,
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
                    // append = &gtk::CheckButton {
                    //     set_sensitive: false,
                    //     set_label: Some("From file"),
                    //     set_group: Some(&group),
                    //     set_active: false,
                    //     connect_toggled[sender] => move |btn| {
                    //     if btn.is_active() {
                    //         sender.input(AppInMsg::SetImageMode(ImageMode::Custom));
                    //     }
                    // }
                    // },

                },


                append = if model.show_button {
                    gtk::Button::with_label("Generate Deck !") {
                        #[watch]
                        set_sensitive: model.sensitive,
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
