// use epub::doc::EpubDoc;
// use getch::Getch;
use rayon::prelude::{ParallelBridge, ParallelIterator};
// use scraper::{Element, Selector};
use srtlib::{Subtitle, Subtitles, Timestamp};
use std::{
    path::PathBuf,
    process::Command,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::Sender,
        Arc,
    },
};
use itertools::Itertools;

const CHUNK_SIZE: usize = 25;
const SILENCE: &[u8] = include_bytes!("../silence.mp3");

// fn timestamp_to_str(t: Timestamp) -> String {
//     let (hours, mins, secs, millis) = t.get();
//     let seconds_total: u64 = u64::from(hours) * 3600 + u64::from(mins) * 60 + u64::from(secs);
//     format!("{}.{:0>3}", seconds_total, millis)
// }

// TODO if we implement that, change rubies from renpy format to anki format
// fn replace_rubies(text: &mut String, rubies: &mut VecDeque<[String; 3]>) {
//     if rubies.front().is_none() {
//         return;
//     }
//     let mut queue: Vec<[String; 2]> = Vec::with_capacity(5);
//     while textdistance::str::overlap(&rubies[0][2], text) > 0.5 && text.contains(&rubies[0][0]) {
//         let front = rubies.pop_front().unwrap();
//         queue.push([front[0].clone(), front[1].clone()]);
//         if rubies.is_empty() {
//             break;
//         }
//     }
//     while let Some(replacement) = queue.pop() {
//         *text = text.replace(
//             &replacement[0].to_string(),
//             &format!(
//                 "{{rb}}{}{{/rb}}{{rt}}{}{{/rt}}",
//                 replacement[0], replacement[1]
//             ),
//         );
//     }
// }
// fn _replace_rubies_old(text: &mut String, rubies: &mut VecDeque<[String; 3]>) {
//     if rubies.front().is_none() {
//         return;
//     }
//     let mut queue: Vec<[String; 2]> = Vec::with_capacity(5);
//     while textdistance::str::overlap(&rubies[0][2], text) > 0.5
//         && text.contains(&format!("{}{}", &rubies[0][0], &rubies[0][1]))
//     {
//         let front = rubies.pop_front().unwrap();
//         queue.push([front[0].clone(), front[1].clone()]);
//         if rubies.is_empty() {
//             break;
//         }
//     }
//     while let Some(replacement) = queue.pop() {
//         *text = text.replace(
//             &format!("{}{}", replacement[0], replacement[1]),
//             &format!(
//                 "{{rb}}{}{{/rb}}{{rt}}{}{{/rt}}",
//                 replacement[0], replacement[1]
//             ),
//         );
//     }
// }

fn prepare_ffmpeg_command(
    start: usize,
    count: usize,
    s: &[Subtitle],
    path: &str,
    prefix: &str,
) -> Vec<String> {
    let mut r = Vec::with_capacity(count * 10);
    for i in 0..count {
        let n = start + i;
        // if s.len() != 25 {
        //     dbg!(n);
        // }
        let path_str = format!("{path}/{prefix}-{n}.mp3");
        let path = PathBuf::from(&path_str);
        if path.exists() {
            //TODO check if file is 0bytes
            // dbg!(n);
            continue;
        }
        if s[i].start_time >= s[i].end_time {
            std::fs::write(&path, SILENCE).unwrap();
            continue;
        }
        r.extend(
            [
                "-c",
                "copy",
                "-ss",
                &s[i].start_time.to_string().replace(',', "."),
                "-to",
                &s[i].end_time.to_string().replace(',', "."),
                &path_str,
            ]
            .map(|s| s.to_string()),
        )
    }
    r
}

#[derive(Debug)]
pub struct MyArgs {
    pub audiobook: PathBuf,
    pub subtitle: PathBuf,
    pub prefix: String,
    // pub epub: Option<String>,
    // pub split: bool,
    // pub show_buggies: bool,
    pub start_offset: i32,
    pub end_offset: i32,
}

pub fn process(args: MyArgs, thread_tx: Sender<String>) {
    // let mut rubies = None;

    // let gch = Getch::new();
    let contin = Arc::new(AtomicBool::new(true));
    // let contin_thread = contin.clone();
    // let contin_ctrlc = contin.clone();

    // ctrlc::set_handler(move || {
    //     println!(
    //         "Shutting gracefully, please wait a moment for the currently converting files to end."
    //     );
    //     contin_ctrlc.store(false, Ordering::Relaxed);
    // })
    // .expect("Error setting Ctrl-C handler");

    // std::thread::spawn(move || loop {
    //     let a = gch.getch().unwrap();
    //     if a == 113 {
    //         println!(
    //         "Shutting gracefully, please wait a moment for the currently converting files to end."
    //     );
    //         contin_thread.store(false, Ordering::Relaxed);
    //     }
    // });

    let mut subs = Subtitles::parse_from_file(&args.subtitle, Some("utf8"))
        .unwrap()
        .to_vec();

    subs.sort();
    let mintime = Timestamp::new(0, 0, 0, args.start_offset.unsigned_abs() as u16);

    let path = format!("./gen/{}/", args.prefix);

    std::fs::create_dir_all(&path).unwrap();

    // Collect all subtitle text into a string.
    let mut subs_strings: Vec<String> = Vec::with_capacity(15000);
    let mut subs2: Vec<Subtitle> = Vec::with_capacity(20000);
    subs.iter().tuple_windows().for_each(|(n, np1)| {
        let mut n2 = n.clone();
        if n.start_time > mintime {
            n2.start_time.add_milliseconds(args.start_offset);
        }
        n2.end_time = np1.start_time;
        n2.end_time.add_milliseconds(args.start_offset);
        subs2.push(n2);
        subs_strings.push(n.text.to_owned());
    });

    subs2.push(subs.last().unwrap().clone());
    subs_strings.push(subs.last().unwrap().text.to_owned());


    let n = AtomicUsize::new(0);
    let m = subs.len();
    // TODO benchmark, audiobook2srs don't care about order
    subs2.chunks(CHUNK_SIZE)
        .enumerate()
        .par_bridge()
        // .par_chunks()
        .for_each(move |(i, s)| {
            let size = s.len();
            let prepared = prepare_ffmpeg_command(i * CHUNK_SIZE, size, s, &path, &args.prefix);
            if !contin.load(Ordering::Relaxed) {
                return;
            }
            let mut command = if cfg!(unix) {
                Command::new("ffmpeg")
            } else if cfg!(windows) {
                Command::new("ffmpeg.exe")
            } else {
                panic!("Unsupported OS possibly.")
            };
            let args: Vec<String> = [
                "-hide_banner".to_string(),
                "-loglevel".to_string(),
                "error".to_string(),
                "-vn".to_string(),
                "-y".to_string(),
                "-i".to_string(),
                args.audiobook.to_string_lossy().to_string(),
            ]
            .iter()
            .chain(prepared.iter())
            .cloned()
            .collect();
            let child = command.args(&args).output().unwrap();
            // dbg!(child);
            n.fetch_add(size, std::sync::atomic::Ordering::Relaxed);
            thread_tx.send(format!("{n:?}/{m} completed!\n")).unwrap();
        })
}
