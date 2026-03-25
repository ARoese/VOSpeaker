use crate::project_dir::hashes::{ConfigHash, VOHash, HASH_LEN};
use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Error, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/*
    # configMap.bin file format specification:
    ## struct pseudocode
    struct ConfigMapFile {
        u32_be hash_length
        HashPair<hash_length>[]
    }

    struct HashPair<L: u32_be> {
        Hash<L> vo_hash
        Hash<L> config_hash
    }

    struct Hash<L: u32_be> {
        u8[L] hash
    }
    ## explanation
    The file type is a simple list of vo_hash -> config_hash mappings. The hash length is stored
    at the start of the file, and the remainder of the file is made out of the pairs. 
    
    The file represents a mapping, but for efficiency reasons, it is possible for the same vo_hash to appear
    multiple times. In this scenario, the last appearance should be considered the valid mapping.
    Writing a new vo_hash mapping to the file does NOT require removing any old
    instances of that vo_hash. Appending the new mapping to the file is sufficient, and a compliant
    implementation will ignore previous appearances when reading.
    
    Reading the file by inserting of each pair into a hashmap in the order they appear in the file
    is a valid procedure.. 
 */

/// squish when this percentage of the entries are duplicates
pub const SQUISH_LOAD: f32 = 0.3;

pub struct ConfigMapFile {
    path: PathBuf,
    file: File,
    map: HashMap<VOHash, ConfigHash>,
    dups: usize
}

impl ConfigMapFile {
    pub fn new(path: &Path) -> Result<Self, Error> {
        let mut opened = OpenOptions::new().read(true).write(true).create(true).open(path)?;
        // exclusive ownership. Do not allow others to access this file while we hold it
        opened.try_lock()?;
        let (dups, map) = read_valid_file(&mut opened)?;
        Ok(ConfigMapFile {
            path: PathBuf::from(path),
            file: opened,
            map,
            dups
        })
    }

    pub fn set_hash(&mut self, vo_hash: &VOHash, config_hash: &ConfigHash) -> Result<(), Error> {
        // TODO: do nothing if the existing config hash for this vo hash is the same as the new one
        write_pair(&mut self.file, vo_hash, config_hash)?;
        self.map.insert(vo_hash.clone(), config_hash.clone())
            .and_then(|_| Some(self.dups+=1));
        if (self.dups as f32 / self.map.len() as f32) > SQUISH_LOAD {
            self.squish()?;
        }
        Ok(())
    }

    pub fn get_hash(&self, vo_hash: &VOHash) -> Option<&ConfigHash> {
        self.map.get(vo_hash)
    }

    /// this leaves the file and struct desynchronized. Callers are expected to resolve this
    fn clear_file(&mut self) -> Result<(), Error> {
        self.file.set_len(0)?;
        self.file.seek(SeekFrom::Start(0))?;
        self.file.write_u32::<BigEndian>(HASH_LEN as u32)?;
        Ok(())
    }

    pub fn clear(&mut self) -> Result<(), Error> {
        self.clear_file()?;
        self.map.clear();
        Ok(())
    }

    pub fn hashes(&self) -> impl Iterator<Item = (&VOHash, &ConfigHash)> + '_ {
        self.map.iter()
    }

    pub fn squish(&mut self) -> Result<(), Error> {
        self.clear_file()?;
        for (vo_hash, config_hash) in &self.map {
            write_pair(&mut self.file, vo_hash, config_hash)?;
        }
        Ok(())
    }
}

fn get_remaining_length(file: &mut File) -> Result<u64, Error> {
    let old_pos = file.stream_position()?;
    file.seek(SeekFrom::End(0))?;
    let remaining_length = file.stream_position()? - old_pos;
    file.seek(SeekFrom::Start(old_pos))?;
    Ok(remaining_length)
}

fn read_pair(file: &mut File) -> Result<(VOHash, ConfigHash), Error>{
    let mut vo_hash = VOHash::default();
    let mut config_hash = ConfigHash::default();
    file.read_exact(&mut vo_hash.0)?;
    file.read_exact(&mut config_hash.0)?;
    Ok((vo_hash, config_hash))
}

fn write_pair(file: &mut File, vo_hash: &VOHash, config_hash: &ConfigHash) -> Result<(), Error> {
    let pair = [vo_hash.0, config_hash.0].concat();
    file.write_all(&pair)?;
    Ok(())
}

/// file will be left with its cursor at the end of the file.
/// it will also contain a valid ConfigMap file
fn read_valid_file(file: &mut File) -> Result<(usize, HashMap<VOHash, ConfigHash>), Error> {
    file.seek(SeekFrom::Start(0))?;
    let hash_len = file.read_u32::<BigEndian>();
    let should_wipe = if let Ok(hash_len) = hash_len {
        hash_len as usize != HASH_LEN
    }else{
        true
    };
    if should_wipe {
        if file.metadata()?.len() != 0 {
            println!("Hash length is invalid. {} expected. Wiping file.", HASH_LEN);
        }
        
        file.set_len(0)?;
        file.seek(SeekFrom::Start(0))?;
        file.write_u32::<BigEndian>(HASH_LEN as u32)?;
        Ok((0, HashMap::new()))
    }else{
        let remaining_bytes = get_remaining_length(file)?;
        let remaining_pairs = remaining_bytes as usize/(HASH_LEN*2);
        let mut map = HashMap::with_capacity(remaining_pairs);

        // this will usually do nothing, but prevents misalignment after the read
        let expected_size_bytes = size_of::<u32>() + remaining_pairs*HASH_LEN*2;
        file.set_len(expected_size_bytes as u64)?;
        let mut dups = 0usize;
        for _ in 0..remaining_pairs {
            let (vo_hash, config_hash) = read_pair(file)?;
            map.insert(vo_hash, config_hash).and_then(|_| Some(dups+=1));
        }
        Ok((dups, map))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read(){
        let test_file_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test_assets/configMap.bin");
        let mut file = ConfigMapFile::new(&test_file_path).unwrap();

        let dummy_vo_hash = VOHash{
            0: [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]
        };
        let dummy_config_hash = ConfigHash{
            0: [15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0]
        };
        file.set_hash(&dummy_vo_hash, &dummy_config_hash).expect("Couldn't set hash");

        println!("{:^32} | {:^32}", "vo_hash", "config_hash");
        println!("{:-^32} | {:-^32}", "", "");
        for (vo_hash, config_hash) in file.hashes() {
            println!("{vo_hash:^32} | {config_hash:^32}")
        }

        //file.squish().expect("Couldn't squish");
    }

    #[test]
    #[should_panic]
    fn test_double_open(){
        let test_file_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test_assets/configMap.bin");
        let file1 = ConfigMapFile::new(&test_file_path).unwrap();
        let file2 = ConfigMapFile::new(&test_file_path).unwrap();
    }
}