use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter, Read, Write};
use util;

#[derive(Serialize)]
struct Range {
    task_no: u64,
    range_start: u64,
    range_end: u64,
}

#[derive(Serialize)]
struct Status {
    url: String,
    file_name: String,
    ranges: Vec<Range>,
}

impl Status {
    fn new(url: &str, file_name: &str) -> Self{
        return Status {
            url: url.to_string(),
            file_name: file_name.to_string(),
            ranges: vec!(),
        }
    }

    fn load_file(file_name: &str) -> Self {
        return Self::new("","")
    }

    fn save_file(self) -> Result<(),String>{
        let file_name = util::add_path_extension(self.file_name.clone(), "toml");
        match File::create(file_name) {
            Ok(mut file) => {
                file.write_all(toml::to_string(&self).unwrap().as_bytes()).unwrap();
                Ok(())
            }
            Err(f) => Err(format!("{}", f))
        }
    }
}
