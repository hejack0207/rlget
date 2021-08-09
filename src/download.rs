extern crate reqwest;
extern crate indicatif;
extern crate http;

use self::indicatif::{MultiProgress, ProgressBar, ProgressStyle};

use self::reqwest::header;
use self::reqwest::{Client, ClientBuilder, Response};
use std::io::prelude::*;
use std::fs::{File, OpenOptions};
use std::io::SeekFrom;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use self::http::header::{HOST, CONTENT_RANGE};

static KB: u64 = 1024;

pub struct Download {
    pub url: String,
    pub filename: String,
    pub memory: u64,
    pub threads: u64,
    pub client: Arc<Client>,
}

impl Default for Download {
    fn default() -> Download {
        Download {
            url: "".to_string(),
            filename: "".to_string(),
            memory: 256,
            threads: 5,
            client: {
                let cb = ClientBuilder::new();
                let client = cb.timeout(Duration::from_secs(60*15)).build().expect("Client::new()");
                Arc::new(client)
            },
        }
    }
}

impl Download {
    pub fn get(&mut self) {
        let mut i = 1;
        loop {
            let content_length_resp : Response = match self.client
                .get(&self.url)
                .send() {
                            Ok(resp) => resp,
                            Err(e) => {
                                println!("error occur when connecting url, retry..., #{}", i);
                                thread::sleep_ms(1000*60*1);
                                continue;
                            }
                        };

            match content_length_resp.content_length() {
                Some(content_length) => {
                    println!("content-length: {}",content_length);
                    let threads = content_length / ( self.memory * KB ) + 1;
                    self.threads = u64::min(self.threads, threads);// if self.threads <= threads { self.threads } else { threads };

                    let children = download_parts(
                        self.client.clone(),
                        self.url.clone(),
                        self.filename.clone(),
                        self.memory,
                        self.threads,
                        content_length);

                    for child in children {
                        let _ = child.join();
                    }
                    break
                }
                None => {
                    println!("content-length not found,try again, {}!",i);
                    thread::sleep_ms(1000*3);
                    ()
                }
            }
            i += 1;
        }
    }
}

fn download_parts(
    client: Arc<Client>,
    url: String,
    filename: String,
    memory: u64,
    threads: u64,
    content_length: u64,
) -> Vec<std::thread::JoinHandle<()>> {
    let mut range_start = 0;
    let mut children = vec![];
    let chunk_size = content_length / threads - 1;

    let m = MultiProgress::new();
    let sty = ProgressStyle::default_bar()
        .template("[{bar:40.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta} remaining)  {msg}")
        .progress_chars("##-");

    println!("Spawning Threads...");
    for thread_id in 0..threads {
        let mut range_end = chunk_size + range_start;
        if thread_id == (threads - 1) {
            range_end = content_length
        }
        let range: String = format!("bytes={}-{}", range_start, range_end);
        let range_to_process: u64 = range_end - range_start;
        let pb = m.add(ProgressBar::new(range_to_process));
        pb.set_style(sty.clone());
        pb.set_message(&format!("thread #{}", thread_id + 1));

        let client_ref = client.clone();
        let mut filename_ref = filename.clone();
        // filename_ref.push_str(&format!(".{}",thread_id + 1));
        let url_ref = url.clone();
        children.push(
            thread::spawn(move || {
                let mut last_chunk = 0;
                let mut range_start = range_start;
                'restart:
                loop {
                    if let Err(e) = download_part(
                        client_ref.clone(),
                        &url_ref,
                        filename_ref.clone(),
                        &pb,
                        memory,
                        &mut range_start,
                        range_end,
                        range_to_process,
                        thread_id,
                        &mut last_chunk,
                        ){
                            println!("{}", e);
                            thread::sleep_ms(1000*60*1);
                            continue 'restart;
                    };
                    break;
                }
            })
        );

        range_start = range_start + chunk_size + 1;
    }
    m.join_and_clear().unwrap();
    return children;
}

fn download_part(
    client_ref: Arc<Client>,
    url_ref: &str,
    filename_ref: String,
    pb: &ProgressBar,
    memory: u64,
    range_start: &mut u64,
    range_end: u64,
    range_to_process: u64,
    thread_id: u64,
    last_chunk: &mut u64,
) -> Result<(), String> {
    let buffer_size = memory * KB;
    let buffer_chunks: u64 = range_to_process / buffer_size;
    let chunk_remainder: u64 = range_to_process % buffer_size;

    // let mut file_handle = File::open(filename_ref.clone()).unwrap_or(File::create(filename_ref.clone()).unwrap());
    let mut file_handle = OpenOptions::new().read(true).write(true).create(true).open(filename_ref).unwrap();
    print_debug_message(
        &format!("thread:{}, range_start:{}",thread_id+1, range_start),
        thread_id+1,
        );
    file_handle.seek(SeekFrom::Start(*range_start)).unwrap();

    let range: String = format!("bytes={}-{}", range_start, range_end);
    let mut file_range_resp : Response = match client_ref
        .get(url_ref)
        .header(header::RANGE, range)
        .send() {
            Ok(resp) => resp,
            Err(_) => return Err(format!("connection timeout, retry..., #{}, continue from {}", thread_id + 1, range_start))
        };

    check_response_error(&file_range_resp, thread_id, *range_start)?;

    for chunk_seq in *last_chunk..buffer_chunks {
        read_chunk_to_file(
            &mut file_range_resp,
            &mut file_handle,
            &mut *range_start,
            &mut *last_chunk,
            pb,
            thread_id,
            chunk_seq,
            buffer_size,
            )?;
    }

    if chunk_remainder != 0 {
        if let Err(_) = file_range_resp.copy_to(&mut file_handle){
            return Err(format!("receive data timeout, retry..., #{}, continue from {}", thread_id + 1, range_start));
        }
    }
    pb.inc(chunk_remainder);
    pb.finish_with_message(&format!("--#{} done--",thread_id + 1));
    return Ok(());
}

fn read_chunk_to_file(
    file_range_resp :&mut Response,
    file_handle :&mut File,
    range_start :&mut u64,
    last_chunk :&mut u64,
    pb :&ProgressBar,
    thread_id :u64,
    chunk_seq :u64,
    chunk_size :u64,
) -> Result<(), String> {

    let mut buffer = vec![0u8; chunk_size as usize];
    let file_range_ref = file_range_resp.by_ref();
    if let Err(_) = file_range_ref.read_exact(&mut buffer) {
        return Err(
            format!("receive data timeout, retry..., #{}, continue from {}:{}",
                thread_id + 1,
                chunk_seq,
                range_start,
                )
            );
    };
    file_handle.write(&buffer).unwrap();

    #[cfg(feature="content_comparing")]
    check_buffer(&buffer, *range_start)?;

    file_handle.flush().unwrap();
    *range_start += chunk_size;
    *last_chunk = chunk_seq + 1;
    pb.inc(chunk_size);
    print_debug_message(
        &format!("debug: sleep {} seconds ...", thread_id + 1),
        thread_id + 1,
        );
    Ok(())
}

fn check_response_error(file_range_resp :&Response, thread_id :u64, range_start :u64) -> Result<(), String> {
    if let Some(content_length) = file_range_resp.content_length() {
        print_debug_message(&format!("content-length:{}",content_length), thread_id + 1);
    }else{
        print_debug_message(&format!("content-length not found"), thread_id + 1);
        return Err(format!("content-length not found, retry..., #{}!", thread_id + 1));
    };

    if let Some(content_range) = file_range_resp.headers().get(CONTENT_RANGE){
        // format like "bytes 0-15439051/77195264","bytes 15439052-30878103/77195264"
        let (mut start, _, _) = parse_content_range_str(content_range.to_str().unwrap());
        if start != range_start {
            return Err(format!("range_start does not match:{}:{},{}", thread_id + 1, start, range_start));
        };
    }else{
        return Err(format!("content range not found: {},{}", thread_id + 1, range_start));
    };
    Ok(())
}

fn print_debug_message(msg: &str, thread_id: u64){
    unsafe {
        if crate::DEBUG {
            println!("{}",msg);
            thread::sleep_ms(900+10*(thread_id) as u32);
        }
    }
}

#[cfg(feature="content_comparing")]
fn check_buffer(buffer : &Vec<u8>, range_start : u64) -> Result<(),String> {
    unsafe {
        let mut origin_file = crate::ORIGINFILE.clone();
        if crate::DEBUG {
            let mut file_handle = File::open(origin_file).unwrap();
            file_handle.seek(SeekFrom::Start(range_start));
            let mut fbuffer = buffer.clone();
            file_handle.read_exact(&mut fbuffer);
            if buffer[0] != fbuffer[0] {
                let err_msg = format!("data received not correct, range_start: {}", range_start);
                println!("{}", err_msg);
                return Err(err_msg);
            }
        }
    }

    Ok(())
}

fn parse_content_range(message: String) -> (usize, usize, usize){
    let (unit, content) = message.split_at(message.find(" ").unwrap());
    let content = content.to_string();
    let (se, length) = content.split_at(content.find("/").unwrap());
    let se = se.to_string();
    let (start, end) = se.split_at(se.find("-").unwrap());
    println!("{},{},{}", start, end, length);
    return (start.parse().unwrap(), end.parse().unwrap(), length.parse().unwrap());
}

fn parse_content_range_str(message: &str) -> (u64, u64, u64){
    let content = message.get((message.find(" ").unwrap()+1)..).unwrap();
    let se = content.get(0..(content.find("/").unwrap())).unwrap();
    let length = content.get((content.find("/").unwrap()+1)..).unwrap();
    let start = se.get(0..(se.find("-").unwrap())).unwrap();
    let end = se.get((se.find("-").unwrap())+1..).unwrap();
    println!("{},{},{}", start, end, length);
    return (start.parse().unwrap(), end.parse().unwrap(), length.parse().unwrap());
}

#[test]
fn test_parse_content_range(){
    let (start, end, length) = parse_content_range("bytes 15439052-30878103/77195264".to_string());
    assert_eq!(15439052, start);
    assert_eq!(30878103, end);
    assert_eq!(77195264, length);
}

#[test]
fn test_parse_content_range_str(){
    let (start, end, length) = parse_content_range_str("bytes 15439052-30878103/77195264");
    assert_eq!(15439052, start);
    assert_eq!(30878103, end);
    assert_eq!(77195264, length);
}

#[test]
fn test_str(){
    assert!("abc".starts_with("ab"));
    assert!("abc".ends_with("bc"));
    assert_eq!("234","0123456789".get(0..5).unwrap().get(2..).unwrap());
}

#[test]
fn test_mut_usize(){
    fn change_to_zero(x : &mut u64){
        *x = 0;
    }
    fn change_to_zerox(mut x : u64){
        x = 0;
    }

    let mut i  = 100;
    change_to_zero(&mut i);
    assert_eq!(0, i);

    let mut i  = 100;
    change_to_zerox(i);
    assert_eq!(100, i);
}
