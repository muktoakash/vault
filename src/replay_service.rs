use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::option::Option;
// use test::Bencher;

// use nom::{le_u8, le_u16, le_u32, IResult};
// use nom::types::CompleteByteSlice;

use nom::IResult;
use nom::branch::{alt};
use nom::bytes::complete::{tag, take};
use nom::combinator::{map, map_res, peek, verify};
use nom::multi::{count, many0};
use nom::number::complete::{le_u8, le_u16, le_u32, le_u64};
use nom::sequence::{preceded, tuple};

use new_replay::NewReplay;
// use parser::{orig_le_u16, g_le_u8, g_le_u16, g_le_u32, g_le_u64, cbs_le_u16, match_utf8, match_version, match_terminated_utf16};

use parser::{count_n, take_n, parse_utf8_fixed, parse_utf16_terminated, take_zeroes, verify_le_u32, verify_zero_u16, parse_utf8_variable, parse_utf16_variable};

const GAME_TYPE: &'static str = "COH2_REC";
const CHUNKY_NAME: &'static str = "Relic Chunky";

const CHUNKY_TYPE: u32 = 0x1A0A0D;
const CHUNKY_VERSION: u32 = 0x3;

pub fn parse(path: &Path) -> bool {
    let mut file = File::open(path).unwrap();
    let mut buff: Vec<u8> = Vec::new();
    file.read_to_end(&mut buff).unwrap();

    // let (remaining, replay) = parse_replay(&buff).unwrap();
    true
}

// fn parse_replay(input: &[u8]) -> IResult<&[u8], NewReplay> {
//     let (input, (
//             version,
//             game_type,
//             timestamp
//         )) = tuple((
//             parse_version,
//             parse_game_type,
//             parse_utf16_terminated
//         ))(input)?;

//     Ok((input, NewReplay::new(version, game_type, timestamp)))
// }

struct Header {
    pub version: u16,
    pub game_type: String,
    pub timestamp: String
}

fn parse_header(input: &[u8]) -> IResult<&[u8], Header> {
    map(
        tuple((
            preceded(verify_zero_u16, le_u16),
            parse_utf8_fixed(8usize),
            parse_utf16_terminated,
            take_zeroes
        )),
        |(
            version,
            game_type,
            timestamp,
            _
        )| {
            Header {
                version,
                timestamp,
                game_type: game_type.to_owned()
            }
        }
    )(input)
}

struct RelicChunky {
    pub name: String,
    pub signature: u32,
    pub major_version: u32,
    pub minor_version: u32, // maybe?
    pub chunk_offset: u32, // bytes from start of chunky to start of first member chunk
    pub unknown_offset: u32, // usually 0x1C
    pub unknown_id: u32, // usually 0x1
    pub chunks: Vec<Box<dyn Chunk>>
}

fn parse_chunky(input: &[u8]) -> IResult<&[u8], RelicChunky> {
    map(
        tuple((
            tag("Relic Chunky"),
            verify_le_u32(0x1A0A0D),
            verify_le_u32(0x3),
            verify_le_u32(0x1),
            le_u32,
            le_u32,
            le_u32,
            many0(parse_chunk)
        )),
        |(
            name,
            signature,
            major_version,
            minor_version,
            chunk_offset,
            unknown_offset,
            unknown_id,
            chunks
        )| {
            RelicChunky {
                name: String::from_utf8_lossy(name).into_owned(),
                signature,
                major_version,
                minor_version,
                chunk_offset,
                unknown_offset,
                unknown_id,
                chunks
            }
        }
    )(input)
}

pub trait Chunk {
    fn test(&self) -> String {
        String::from("test")
    }
}

#[derive(Debug, Clone)]
struct ChunkHeader {
    pub chunk_kind: String,
    pub chunk_type: String,
    pub version: u32,
    pub length: u32,
    pub name_length: u32,
    pub min_version: u32, // according to Copernicus
    pub flags: u32 // according to Copernicus
}

fn parse_chunk(input: &[u8]) -> IResult<&[u8], Box<dyn Chunk>> {
    alt((
        parse_data_chunk, parse_folder_chunk
    ))(input)
}

struct FOLDChunk {
    pub header: ChunkHeader,
    pub chunks: Vec<Box<dyn Chunk>>
}

impl Chunk for FOLDChunk {}

fn parse_folder_chunk(input: &[u8]) -> IResult<&[u8], Box<dyn Chunk>> {
    map(
        preceded(
            peek(tag("FOLD")),
            tuple((
                parse_chunk_header,
                many0(parse_chunk)
            ))
        ),
        |(
            header,
            chunks
        )| {
            Box::new(FOLDChunk {
                header,
                chunks
            }) as Box<dyn Chunk>
        }
    )(input)
}

fn parse_data_chunk(input: &[u8]) -> IResult<&[u8], Box<dyn Chunk>> {
    preceded(
        peek(tag("DATA")),
        alt((
            parse_datasdsc_chunk,
            parse_datadata_chunk,
            parse_dataplas_chunk
        ))
    )(input)
}

struct SimpleDATADATAChunk {
    pub header: ChunkHeader,
    pub unknown: Vec<u8>
}

impl Chunk for SimpleDATADATAChunk {}

struct ComplexDATADATAChunk {
    pub header: ChunkHeader,
    pub opponent_type: u32,
    pub unknown_flag_1: u32, // 0 or 1
    pub unknown_flag_2: u32, // 0
    pub unknown_flag_3: u16, // 0
    pub rng_seed: u32,
    pub player_count: u32,
    pub player_data: Vec<PlayerData>
}

impl Chunk for ComplexDATADATAChunk {}

fn parse_datadata_chunk(input: &[u8]) -> IResult<&[u8], Box<dyn Chunk>> {
    let (input, header) = preceded(peek(tag("DATADATA")), parse_chunk_header)(input)?;

    match header.version {
        1 => parse_simple_datadata_chunk(header)(input),
        _ => parse_complex_datadata_chunk(header)(input)
    }
}

fn parse_simple_datadata_chunk<'a>(header: ChunkHeader) -> impl Fn(&'a [u8]) -> IResult<&'a [u8], Box<dyn Chunk>> {
    map(
        take(header.length),
        move |unknown: &[u8]| {
            Box::new(SimpleDATADATAChunk {
                header: header.clone(),
                unknown: unknown.to_vec()
            }) as Box<dyn Chunk>
        }
    )
}

fn parse_complex_datadata_chunk<'a>(header: ChunkHeader) -> impl Fn(&'a [u8]) -> IResult<&'a [u8], Box<dyn Chunk>> {
    map(
        tuple((
            le_u32,
            le_u32,
            le_u32,
            le_u16,
            le_u32,
            count_n(le_u32, parse_player_data)
        )),
        move |(
            opponent_type,
            unknown_flag_1,
            unknown_flag_2,
            unknown_flag_3,
            rng_seed,
            (player_count, player_data)
        )| {
            Box::new(ComplexDATADATAChunk {
                header: header.clone(),
                opponent_type,
                unknown_flag_1,
                unknown_flag_2,
                unknown_flag_3,
                rng_seed,
                player_count,
                player_data
            }) as Box<dyn Chunk>
        }
    )
}

struct PlayerData {
    pub unknown_flag_1: u8, // could be 1 = human player, 0 = cpu player?
    pub name_length: u32,
    pub name: String,
    pub team: u32,
    pub faction_length: u32,
    pub faction: String,
    pub unknown_flag_2: u32, // 5 for army type
    pub unknown_flag_3: u32, // Seb: p00
    pub game_mode_length: u32,
    pub game_mode: String, // Seb: default or skirmish
    pub unknown_flag_4: u32, // Seb: this is not count, it's t1p1 t2p1 t1p2 t2p2 etc
                             // (fixed pos) or I dont even know anymore (for random)
                             // its still count
    pub unknown_flag_5: u32, // something (not position)
    pub unknown_flag_6: u32, // 0x0
    pub unknown_flag_7: u32, // 0x5
    pub unknown_flag_8: u16, // 0x1 - not sure what this is yet
    pub unknown_flag_9: u16, // 0x1 - not sure what this is yet
    pub unknown_flag_10: u64, // u64::MAX if cpu and no steam id, but it will return
                              // 0 in this case so just read anyways
    pub steam_id: u64,
    pub item_block_1_size: u32, // commanders are usually in this block
    pub item_block_2_size: u32, // bulletins are usually in this block
    pub unknown_flag_11: u32, // 0x0
    pub unknown_flag_12: u32, // don't know what this is yet, 2 u32s
    pub unknown_flag_13: u32, // ^
    pub item_data: Vec<Box<dyn ItemData>>
}

fn parse_player_data(input: &[u8]) -> IResult<&[u8], PlayerData> {
    map(
        tuple((
            le_u8,
            parse_utf16_variable(le_u32),
            le_u32,
            parse_utf8_variable(le_u32),
            le_u32,
            le_u32,
            parse_utf8_variable(le_u32),
            le_u32,
            le_u32,
            le_u32,
            le_u32,
            le_u16,
            count(parse_item_data, 3),
            le_u16,
            le_u64,
            le_u64,
            count(parse_item_data, 3),
            count_n(le_u32, parse_item_data),
            count_n(le_u32, parse_item_data),
            le_u32,
            tuple((
                le_u32,
                le_u32
            ))
        )),
        |(
            unknown_flag_1,
            (name_length, name),
            team,
            (faction_length, faction),
            unknown_flag_2,
            unknown_flag_3,
            (game_mode_length, game_mode),
            unknown_flag_4,
            unknown_flag_5,
            unknown_flag_6,
            unknown_flag_7,
            unknown_flag_8,
            item_data,
            unknown_flag_9,
            unknown_flag_10,
            steam_id,
            other_item_data,
            (item_block_1_size, item_block_1),
            (item_block_2_size, item_block_2),
            unknown_flag_11,
            (
                unknown_flag_12,
                unknown_flag_13
            )
        )| {
            let items = vec![item_data, other_item_data, item_block_1, item_block_2];

            PlayerData {
                unknown_flag_1,
                name_length,
                name,
                team,
                faction_length,
                faction,
                unknown_flag_2,
                unknown_flag_3,
                game_mode_length,
                game_mode,
                unknown_flag_4,
                unknown_flag_5,
                unknown_flag_6,
                unknown_flag_7,
                unknown_flag_8,
                unknown_flag_9,
                unknown_flag_10,
                steam_id,
                item_block_1_size,
                item_block_2_size,
                unknown_flag_11,
                unknown_flag_12,
                unknown_flag_13,
                item_data: items.into_iter().flatten().collect()
            }
        }
    )(input)
}

pub trait ItemData {
    fn test(&self) -> String {
        String::from("test")
    }
}

fn parse_item_data(input: &[u8]) -> IResult<&[u8], Box<dyn ItemData>> {
    alt((
        parse_player_item_data,
        parse_special_player_item_data,
        parse_cpu_item_data
    ))(input)
}

struct PlayerItemData {
    pub item_type: u16,
    pub selection_id: u32,
    pub unknown_flag_1: u32, // 0x0
    pub server_id: u32,
    pub unknown_flag_2: u32, // 0x0
    pub remaining_buffer_size: u16,
    pub remaining_buffer: Vec<u8>
}

impl ItemData for PlayerItemData {}

fn parse_player_item_data(input: &[u8]) -> IResult<&[u8], Box<dyn ItemData>> {
    map(
        tuple((
            verify(le_u16, |n: &u16| *n == 0x109),
            le_u32,
            le_u32,
            le_u32,
            le_u32,
            take_n(le_u16)
        )),
        |(
            item_type,
            selection_id,
            unknown_flag_1,
            server_id,
            unknown_flag_2,
            (remaining_buffer_size, remaining_buffer)
        )| {
            Box::new(PlayerItemData {
                item_type,
                selection_id,
                unknown_flag_1,
                server_id,
                unknown_flag_2,
                remaining_buffer_size,
                remaining_buffer: remaining_buffer.to_vec()
            }) as Box<dyn ItemData>
        }
    )(input)
}

struct SpecialPlayerItemData {
    pub item_type: u16,
    pub unknown_data: Vec<u8>, // lots of data, no idea what it is
    pub unknown_flag_1: u32, // something to do with custom decals
    pub unknown_flag_2: u8 // not sure, was 0x40 in test replay
}

impl ItemData for SpecialPlayerItemData {}

fn parse_special_player_item_data(input: &[u8]) -> IResult<&[u8], Box<dyn ItemData>> {
    map(
        tuple((
            verify(le_u16, |n: &u16| *n == 0x216),
            take(16usize),
            le_u32,
            le_u8
        )),
        |(
            item_type,
            unknown_data,
            unknown_flag_1,
            unknown_flag_2
        )| {
            Box::new(SpecialPlayerItemData {
                item_type,
                unknown_data: unknown_data.to_vec(),
                unknown_flag_1,
                unknown_flag_2
            }) as Box<dyn ItemData>
        }
    )(input)
}

struct CPUItemData {
    pub item_type: u16,
    pub unknown_flag_1: u8, // 0x1
    pub unknown_flag_2: u32 // gotta figure out what this is
}

impl ItemData for CPUItemData {}

fn parse_cpu_item_data(input: &[u8]) -> IResult<&[u8], Box<dyn ItemData>> {
    map(
        tuple((
            verify(le_u16, |n: &u16| *n == 0x1),
            le_u8,
            le_u32
        )),
        |(
            item_type,
            unknown_flag_1,
            unknown_flag_2
        )| {
            Box::new(CPUItemData {
                item_type,
                unknown_flag_1,
                unknown_flag_2
            }) as Box<dyn ItemData>
        }
    )(input)
}

struct DATASDSCChunk {
    pub header: ChunkHeader,
    pub unknown_flag_1: u32, // 0x0
    pub unknown_flag_2: u32, // 0x0
    pub unknown_flag_3: u32, // can be 1 or 2?
    pub unknown_flag_4: u32, // 0x3
    pub unknown_flag_5: u32, // 0x0
    pub unknown_flag_6: u32, // 0x0
    pub unknown_flag_7: u32, // 0x0
    pub map_file_length: u32,
    pub map_file: String,
    pub unknown_data_1: Vec<u8>, // something to do with map start positions?
    pub map_name_length: u32,
    pub map_name: String,
    pub long_map_description_length: u32,
    pub long_map_description: String,
    pub short_map_description_length: u32,
    pub short_map_description: String,
    pub map_players: u32,
    pub map_width: u32,
    pub map_height: u32,
    pub unknown_data_2: Vec<u8>,
    pub unknown_flag_8: u32, // 0x2?
    pub unknown_data_3: Vec<u8>,
    pub unknown_flag_9: u32, // 0x4?
    pub unknown_data_4_length: u32,
    pub unknown_data_4: String
}

impl Chunk for DATASDSCChunk {}

fn parse_datasdsc_chunk(input: &[u8]) -> IResult<&[u8], Box<dyn Chunk>> {
    map(

        tuple((
            preceded(
                peek(tag("DATASDSC")),
                parse_chunk_header
            ),
            le_u32,
            le_u32,
            le_u32,
            le_u32,
            le_u32,
            le_u32,
            le_u32,
            parse_utf8_variable(le_u32),
            take(16usize),
            parse_utf16_variable(le_u32),
            parse_utf16_variable(le_u32),
            parse_utf16_variable(le_u32),
            le_u32,
            le_u32,
            le_u32,
            take(40usize),
            le_u32,
            take(18usize),
            le_u32,
            parse_utf8_variable(le_u32)
        )),
        |(
            header,
            unknown_flag_1,
            unknown_flag_2,
            unknown_flag_3,
            unknown_flag_4,
            unknown_flag_5,
            unknown_flag_6,
            unknown_flag_7,
            (map_file_length, map_file),
            unknown_data_1,
            (map_name_length, map_name),
            (long_map_description_length, long_map_description),
            (short_map_description_length, short_map_description),
            map_players,
            map_width,
            map_height,
            unknown_data_2,
            unknown_flag_8,
            unknown_data_3,
            unknown_flag_9,
            (unknown_data_4_length, unknown_data_4)
        )| {
            Box::new(DATASDSCChunk {
                header,
                unknown_flag_1,
                unknown_flag_2,
                unknown_flag_3,
                unknown_flag_4,
                unknown_flag_5,
                unknown_flag_6,
                unknown_flag_7,
                map_file_length,
                map_file,
                unknown_data_1: unknown_data_1.to_vec(),
                map_name_length,
                map_name,
                long_map_description_length,
                long_map_description,
                short_map_description_length,
                short_map_description,
                map_players,
                map_width,
                map_height,
                unknown_data_2: unknown_data_2.to_vec(),
                unknown_flag_8,
                unknown_data_3: unknown_data_3.to_vec(),
                unknown_flag_9,
                unknown_data_4_length,
                unknown_data_4
            }) as Box<dyn Chunk>
        }
    )(input)
}

fn parse_dataplas_chunk(input: &[u8]) -> IResult<&[u8], Box<dyn Chunk>> {
    map(
        tuple((
            tag("DATA"),
            tag("PLAS")
        )),
        |(
            chunk_kind,
            chunk_type
        )| {
            RelicChunk {
                chunk_kind: String::from_utf8_lossy(chunk_kind).into_owned(),
                chunk_type: String::from_utf8_lossy(chunk_type).into_owned()
            }
        }
    )(input)
}

// struct ChunkHeader {
//     pub chunk_kind: String,
//     pub chunk_type: String,
//     pub version: u32,
//     pub length: u32,
//     pub name_length: u32,
//     pub min_version: u32, // according to Copernicus
//     pub flags: u32 // according to Copernicus
// }

fn parse_chunk_header(input: &[u8]) -> IResult<&[u8], ChunkHeader> {
    map(
        tuple((
            parse_utf8_fixed(4),
            parse_utf8_fixed(4),
            le_u32,
            le_u32,
            le_u32,
            le_u32,
            le_u32
        )),
        |(
            chunk_kind,
            chunk_type,
            version,
            length,
            name_length,
            min_version,
            flags
        )| {
            ChunkHeader {
                chunk_kind: chunk_kind.to_owned(),
                chunk_type: chunk_type.to_owned(),
                version,
                length,
                name_length,
                min_version,
                flags
            }
        }
    )(input)
}

// named!(parse_header<(u16, &str, String)>,
//     do_parse!(
//         version: match_version >>
//         game_type: apply!(match_utf8, GAME_TYPE) >>
//         timestamp: match_terminated_utf16 >>
//         many_m_n!(7, 7, verify!(le_u32, |n: u32| n == 0)) >>
//         (version, game_type, timestamp)
//     )
// );

// named!(parse_chunky<bool>,
//     do_parse!(
//         apply!(match_utf8, CHUNKY_NAME) >>
//         verify!(le_u32, |n| n == CHUNKY_TYPE) >>
//         verify!(le_u32, |n| n == CHUNKY_VERSION) >>
//         verify!(le_u32, |n| n == 0x1) >>
//         verify!(le_u32, |n| n == 0x24) >>
//         verify!(le_u32, |n| n == 0x1C) >>
//         verify!(le_u32, |n| n == 0x1) >>
//         (true)
//     )
// );

// named!(test_eof<CompleteByteSlice, bool>,
//     do_parse!(
//         // many_till!(g_le_u16, eof!()) >>
//         count!(g_le_u8, 2000000) >>
//         (true)
//     )
// );

// named!(test_eof_slice<bool>,
//     do_parse!(
//         count!(g_le_u8, 2000000) >>
//         (true)
//     )
// );

// // fn test_eof_long(input: CompleteByteSlice) -> IResult<CompleteByteSlice, CompleteByteSlice> {
// //     many_till!(input, take!(1), eof!())
// // }

// #[cfg(test)]
// mod tests {
//     use super::*;

//     #[test]
//     fn test_parse_header_ok() {
//         let buff = read_into_buffer(Path::new("/Users/ryantaylor/Code/vault/replays/bench.rec"));
//         let (_, (version, game_type, timestamp)) = parse_header(&buff).unwrap();
//         assert_eq!(version, 20297);
//         assert_eq!(game_type, "COH2_REC");
//         assert_eq!(timestamp, "11/7/2015 1:16 AM");
//     }

//     #[test]
//     fn test_parse_chunky_ok() {
//         let buff = read_into_buffer(Path::new("/Users/ryantaylor/Code/vault/replays/bench.rec"));
//         let (remaining, _) = parse_header(&buff).unwrap();
//         let (_, result) = parse_chunky(remaining).unwrap();
//         assert!(result);
//     }

//     #[test]
//     fn test_parse_eof() {
//         let buff = read_into_buffer(Path::new("/Users/ryantaylor/Code/vault/replays/bench.rec"));
//         let (remaining, _) = test_eof(CompleteByteSlice(&buff)).unwrap();
//     }

//     #[bench]
//     fn bench_parse_eof(b: &mut Bencher) {
//         let buff = read_into_buffer(Path::new("/Users/ryantaylor/Code/vault/replays/bench.rec"));
//         b.iter(|| {
//             test_eof(CompleteByteSlice(&buff)).unwrap()
//             // println!("{:?}", remaining);
//             // println!("{:?}", val);
//         });
//     }

//     #[bench]
//     fn bench_parse_eof_slice(b: &mut Bencher) {
//         let buff = read_into_buffer(Path::new("/Users/ryantaylor/Code/vault/replays/bench.rec"));
//         b.iter(|| {
//             test_eof_slice(&buff).unwrap()
//             // println!("{:?}", remaining);
//         });
//     }

//     fn read_into_buffer(path: &Path) -> Vec<u8> {
//         let mut file = File::open(path).unwrap();
//         let mut buff: Vec<u8> = Vec::new();
//         file.read_to_end(&mut buff).unwrap();
//         buff
//     }
// }
