use std::collections::HashMap;
use types::{Error, U5, VecU5};
use bech32::Bech32;
use byteorder::{BigEndian, ByteOrder, WriteBytesExt};
use itertools::Itertools;
use utils::from_hex;

/// Bech32 alphabet
lazy_static! {
    static ref BECH32_ALPHABET: HashMap<char, u8> =
        hashmap!['q' => 0,'p' => 1,'z' => 2,'r' => 3,'y' => 4,'9' => 5,'x' => 6,'8' => 7,'g' => 8,
        'f' => 9,'2' => 10,'t' => 11,'v' => 12,'d' => 13,'w' => 14,'0' => 15,'s' => 16,'3' => 17,
        'j' => 18,'n' => 19,'5' => 20,'4' => 21,'k' => 22,'h' => 23, 'c' => 24, 'e' => 25,'6' => 26,
        'm' => 27,'u' => 28,'a' => 29,'7' => 30,'l' => 31];
}

/// Bitcoin subunits
/// The following **multiplier** letters are defined:
///
/// 'm' (milli): multiply by 0.001
/// 'u' (micro): multiply by 0.000001
/// 'n' (nano): multiply by 0.000000001
/// 'p' (pico): multiply by 0.000000000001
///
pub struct Unit;

impl Unit {
    /// value corresponding to a given letter
    pub fn value(c: char) -> f64 {
        match c {
            'p' => 1000_000_000_000f64,
            'n' => 1000_000_000f64,
            'u' => 1000_000f64,
            'm' => 1000f64,
            _ => 1f64,
        }
    }
    /// multiplier letters
    pub fn units<'a>() -> &'a [&'a str] {
        &["p", "n", "u", "m"]
    }
}

/// BOLT #11:

/// Given an amount in bitcoin, shorten it
///
/// BOLT #11:
/// A writer MUST encode `amount` as a positive decimal integer with no
/// leading zeroes, SHOULD use the shortest representation possible.
pub fn encode_amount(amount: f64) -> String {
    let units = Unit::units();
    // convert to pico initially
    let pico_amount = (amount * Unit::value('p')) as u64;
    encode_amount_aux(pico_amount, &units)
}

fn encode_amount_aux(amount: u64, units: &[&str]) -> String {
    if units.len() == 0 {
        amount.to_string()
    } else if amount % 1000 == 0 {
        encode_amount_aux(amount / 1000, &units[1..])
    } else {
        amount.to_string() + units[0]
    }
}

/// Given an encoded amount, convert it into a decimal
/// BOLT #11:
/// A reader SHOULD fail if `amount` contains a non-digit, or is followed by
/// anything except a `multiplier` in the table above.
/// # Arguments
/// * `amount` - A string that holds the amount to shorten
pub fn decode_amount(amount: &str) -> Result<f64, Error> {
    let unit_char = amount.chars().last().map(|c| Unit::value(c));

    match unit_char {
        Some(u) if u != 1f64 => amount[..amount.len() - 1].parse::<f64>().map(|v| v / u),
        _ => amount.parse::<f64>(),
    }.map_err(Error::ParseFloatErr)
}

#[derive(Debug, Eq, PartialEq)]
/// Tag
pub enum Tag {
    /// Payment Hash Tag
    ///
    /// # Arguments
    /// * `hash` payment hash
    PaymentHash { hash: Vec<u8> },

    /// Description Tag
    ///
    /// # Arguments
    /// * `description` a free-format string that will be included in the payment request
    Description { description: String },

    /// Hash Tag
    ///
    /// # Arguments
    /// `hash` hash that will be included in the payment request, and can be checked against
    ///  the hash of a long description, an invoice, ...
    DescriptionHash { hash: Vec<u8> },

    /// Fallback Payment Tag that specifies a fallback payment address to be used if LN payment
    /// cannot be processed
    ///
    /// # Arguments
    /// `version` address version; valid values are
    ///               - 17 (pubkey hash)
    ///               - 18 (script hash)
    ///               - 0 (segwit hash: p2wpkh (20 bytes) or p2wsh (32 bytes))
    /// `hash`    address hash
    ///
    FallbackAddress { version: u8, hash: Vec<u8> },

    /// Expiry Date
    ///
    /// # Arguments
    /// `seconds` expiry data for this payment request
    Expiry { seconds: u64 },

    /// Min final CLTV expiry
    ///
    /// `blocks` min final cltv expiry, in blocks
    MinFinalCltvExpiry { blocks: u64 },

    /// Routing Info Tag
    ///
    /// # Arguments
    ///  `path` one or more entries containing extra routing information for a private route
    RoutingInfo { path: Vec<ExtraHop> },

    /// unknown tag
    UnknownTag { tag: U5, bytes: Vec<U5> },
}

impl Tag {
    /// convert to u5 vector
    pub fn to_vec_u5(&self) -> Result<Vec<U5>, Error> {
        match &self {
            &&Tag::PaymentHash { ref hash } => {
                let bytes = VecU5::from_u8_vec(hash);
                let p = BECH32_ALPHABET[&'p'];
                Tag::to_vec_u5_convert(p, bytes)
            }
            &&Tag::Description { ref description } => {
                let bytes = VecU5::from_u8_vec(&description.as_bytes().to_vec());
                let d = BECH32_ALPHABET[&'d'];
                Tag::to_vec_u5_convert(d, bytes)
            }
            &&Tag::DescriptionHash { ref hash } => {
                let bytes = VecU5::from_u8_vec(hash);
                let h = BECH32_ALPHABET[&'h'];
                Tag::to_vec_u5_convert(h, bytes)
            }
            &&Tag::FallbackAddress { version, ref hash } => {
                let bytes = VecU5::from_u8_vec(hash).map(|b| {
                    let mut data = vec![version];
                    data.extend(b);
                    data
                });
                let f = BECH32_ALPHABET[&'f'];
                Tag::to_vec_u5_convert(f, bytes)
            }
            &&Tag::Expiry { seconds } => {
                let bytes = VecU5::from_u64(seconds);
                let x = BECH32_ALPHABET[&'x'];
                Tag::write_size(bytes.len()).map(|size| [vec![x], size, bytes].concat())
            }
            &&Tag::MinFinalCltvExpiry { blocks } => {
                let bytes = VecU5::from_u64(blocks);
                let c = BECH32_ALPHABET[&'c'];
                Tag::write_size(bytes.len()).map(|size| [vec![c], size, bytes].concat())
            }
            &&Tag::RoutingInfo { ref path } => {
                let bytes = path.iter()
                    .map(|hop| hop.pack())
                    .fold_results(Vec::<u8>::new(), |mut acc, hop| {
                        acc.extend(hop);
                        acc
                    })
                    .and_then(|v| VecU5::from_u8_vec(&v));

                let r = BECH32_ALPHABET[&'r'];
                Tag::to_vec_u5_convert(r, bytes)
            }
            &&Tag::UnknownTag { tag, ref bytes } => Tag::write_size(bytes.len())
                .map(|size| [vec![tag], size, bytes.to_owned()].concat()),
        }
    }
    // helper for to_vec_u5
    fn to_vec_u5_convert(value: u8, data: Result<Vec<u8>, Error>) -> Result<Vec<U5>, Error> {
        match data {
            Ok(bytes) => {
                let len = bytes.len();
                let mut vec = vec![value, (len / 32) as u8, (len % 32) as u8];
                vec.extend(bytes);
                Ok(vec)
            }
            Err(err) => Err(err),
        }
    }

    fn write_size(size: usize) -> Result<Vec<U5>, Error> {
        let output = VecU5::from_u64(size as u64);
        match output.len() {
            0 => Ok(vec![0u8, 0]),
            1 => Ok([vec![0u8], output].concat()),
            2 => Ok(output),
            _ => Err(Error::InvalidLength(String::from(
                "tag data length field must be encoded on 2 5-bits u8",
            ))),
        }
    }
}

impl Tag {
    pub fn parse(input: &Vec<U5>) -> Result<Tag, Error> {
        let tag = input[0];
        let len = (input[1] * 32 + input[2]) as usize;
        match tag {
            p if p == BECH32_ALPHABET[&'p'] => {
                let hash_result = VecU5::to_u8_vec(&input[3..55].to_vec());
                hash_result.map(|hash| Tag::PaymentHash { hash })
            }
            d if d == BECH32_ALPHABET[&'d'] => {
                let description_result = VecU5::to_u8_vec(&input[3..len + 3].to_vec());
                description_result
                    .and_then(|v| String::from_utf8(v).map_err(Error::FromUTF8Err))
                    .map(|description| Tag::Description { description })
            }
            h if h == BECH32_ALPHABET[&'h'] => {
                let hash_result = VecU5::to_u8_vec(&input[3..len + 3].to_vec());
                hash_result.map(|hash| Tag::DescriptionHash { hash })
            }
            f if f == BECH32_ALPHABET[&'f'] => {
                let version = input[3];
                let hash_result = VecU5::to_u8_vec(&input[4..len + 3].to_vec());
                match version {
                    v if v <= 18u8 => {
                        hash_result.map(|hash| Tag::FallbackAddress { version, hash })
                    }
                    _ => Ok(Tag::UnknownTag {
                        tag,
                        bytes: input[3..len + 3].to_vec(),
                    }),
                }
            }
            r if r == BECH32_ALPHABET[&'r'] => {
                let data_result = VecU5::to_u8_vec(&input[3..len + 3].to_vec());
                data_result
                    .map(ExtraHop::parse_all)
                    .map(|path| Tag::RoutingInfo { path })
            }
            x if x == BECH32_ALPHABET[&'x'] => {
                let seconds = VecU5::to_u64(len, &input[3..len + 3].to_vec());
                Ok(Tag::Expiry { seconds })
            }
            c if c == BECH32_ALPHABET[&'c'] => {
                let blocks = VecU5::to_u64(len, &input[3..len + 3].to_vec());
                Ok(Tag::MinFinalCltvExpiry { blocks })
            }
            _ => Ok(Tag::UnknownTag {
                tag,
                bytes: input[3..len + 3].to_vec(),
            }),
        }
    }
}

/// seconds-since-1970 (35 bits, big-endian)
struct Timestamp;

impl Timestamp {
    /// decode timestamp from u5 vector
    fn decode(data: &Vec<U5>) -> u64 {
        data.iter().take(7).fold(0, |a, b| a * 32u64 + *b as u64)
    }
    /// encode timestamp
    fn encode(timestamp: u64) -> Vec<U5> {
        let mut acc: Vec<U5> = Vec::new();
        let mut time_acc = timestamp;
        // 35 bits, big-endian
        while acc.len() < 7 {
            acc.push((time_acc % 32) as U5);
            time_acc /= 32;
        }
        acc.reverse();
        acc
    }
}

#[derive(Debug, Eq, PartialEq)]
/// entries containing extra routing information for a private route
pub struct ExtraHop {
    /// public key (264 bits)
    pub_key: Vec<u8>,
    short_channel_id: u64,
    /// big endian
    fee_base_msat: u32,
    /// big endian
    fee_proportional_millionths: u32,
    /// big endian
    cltv_expiry_delta: u16,
}

impl ExtraHop {
    /// 33 + 8 + 4 + 4 + 2
    pub const CHUNK_LENGTH: usize = 51;

    /// pack into Vec<u8>
    pub fn pack(&self) -> Result<Vec<u8>, Error> {
        let mut wtr: Vec<u8> = vec![];
        wtr.write_u64::<BigEndian>(self.short_channel_id)?;
        wtr.write_u32::<BigEndian>(self.fee_base_msat)?;
        wtr.write_u32::<BigEndian>(self.fee_proportional_millionths)?;
        wtr.write_u16::<BigEndian>(self.cltv_expiry_delta)?;
        Ok([self.pub_key.to_owned(), wtr].concat())
    }

    /// parse u8 slice into ExtraHop
    pub fn parse(data: &[u8]) -> ExtraHop {
        let pub_key = data[0..33].to_owned();
        let short_channel_id = BigEndian::read_u64(&data[33..41]);
        let fee_base_msat = BigEndian::read_u32(&data[41..45]);
        let fee_proportional_millionths = BigEndian::read_u32(&data[45..49]);
        let cltv_expiry_delta = BigEndian::read_u16(&data[49..ExtraHop::CHUNK_LENGTH]);
        ExtraHop {
            pub_key,
            short_channel_id,
            fee_base_msat,
            fee_proportional_millionths,
            cltv_expiry_delta,
        }
    }

    /// parse a vec<u8> into a vec<ExtraHop>
    pub fn parse_all(data: Vec<u8>) -> Vec<ExtraHop> {
        data
            .chunks(ExtraHop::CHUNK_LENGTH)
            // the last chunk may be shorter if there's not enough elements
            .filter(|c| c.len() == ExtraHop::CHUNK_LENGTH)
            .map(ExtraHop::parse)
            .collect_vec()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn payment_hash_tag_test() {
        let payment_hash_tag = Tag::PaymentHash {
            hash: vec![
                0u8, 1, 2, 3, 4, 5, 6, 7, 8, 9, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 0, 1, 2, 3, 4, 5, 6,
                7, 8, 9, 1, 2,
            ],
        };
        let u5_hash_tag = vec![
            1u8, 1, 20, 0, 0, 0, 16, 4, 0, 24, 4, 0, 20, 3, 0, 14, 2, 0, 9, 0, 0, 0, 16, 4, 0, 24,
            4, 0, 20, 3, 0, 14, 2, 0, 9, 0, 0, 0, 16, 4, 0, 24, 4, 0, 20, 3, 0, 14, 2, 0, 9, 0, 4,
            1, 0,
        ];
        assert!(payment_hash_tag.to_vec_u5().unwrap().eq(&u5_hash_tag))
    }

    #[test]
    fn description_tag_test() {
        let description_tag = Tag::Description {
            description: "Please consider supporting this project".to_owned(),
        };
        let u5_description_tag = vec![
            13, 1, 31, 10, 1, 22, 6, 10, 24, 11, 19, 12, 20, 16, 6, 6, 27, 27, 14, 14, 13, 20, 22,
            8, 25, 11, 18, 4, 1, 25, 23, 10, 28, 3, 16, 13, 29, 25, 7, 8, 26, 11, 14, 12, 28, 16,
            7, 8, 26, 3, 9, 14, 12, 16, 7, 0, 28, 19, 15, 13, 9, 18, 22, 6, 29, 0,
        ];
        assert!(description_tag.to_vec_u5().unwrap().eq(&u5_description_tag))
    }

    #[test]
    fn description_hash_tag_test() {
        let description_hash_tag = Tag::DescriptionHash {
            hash: vec![
                57u8, 37, 182, 246, 126, 44, 52, 0, 54, 237, 18, 9, 61, 212, 78, 3, 104, 223, 27,
                110, 162, 108, 83, 219, 228, 129, 31, 88, 253, 93, 184, 193,
            ],
        };
        let u5_description_hash_tag = vec![
            23, 1, 20, 7, 4, 18, 27, 13, 29, 19, 30, 5, 16, 26, 0, 0, 13, 23, 13, 2, 8, 4, 19, 27,
            21, 2, 14, 0, 13, 20, 13, 30, 6, 27, 14, 20, 9, 22, 5, 7, 22, 31, 4, 16, 4, 15, 21, 17,
            31, 10, 29, 23, 3, 0, 16,
        ];
        assert!(
            description_hash_tag
                .to_vec_u5()
                .unwrap()
                .eq(&u5_description_hash_tag)
        )
    }

    #[test]
    fn fallback_address_tag_test() {
        let fallback_address_tag = Tag::FallbackAddress {
            version: 17,
            hash: vec![
                49u8, 114, 181, 101, 79, 102, 131, 200, 251, 20, 105, 89, 211, 71, 206, 48, 60,
                174, 76, 167,
            ],
        };

        let u5_fallback_address_tag = vec![
            9, 1, 1, 17, 6, 5, 25, 11, 10, 25, 10, 15, 12, 26, 1, 28, 17, 30, 24, 20, 13, 5, 12,
            29, 6, 17, 30, 14, 6, 0, 30, 10, 28, 19, 5, 7,
        ];
        assert!(
            fallback_address_tag
                .to_vec_u5()
                .unwrap()
                .eq(&u5_fallback_address_tag)
        );
    }

    #[test]
    fn expiry_tag_test() {
        let expiry_tag = Tag::Expiry { seconds: 60 };
        let u5_expiry_tag = &vec![6u8, 0, 2, 1, 28];
        assert!(expiry_tag.to_vec_u5().unwrap().eq(u5_expiry_tag))
    }

    #[test]
    fn min_final_cltv_expiry_tag_test() {
        let min_final = Tag::MinFinalCltvExpiry { blocks: 12 };
        let u5_min_final_tag = &vec![24u8, 0, 1, 12];
        assert!(min_final.to_vec_u5().unwrap().eq(u5_min_final_tag))
    }

    #[test]
    fn routing_info_tag_test() {
        let routing_info = Tag::RoutingInfo {
            path: vec![
                ExtraHop {
                    pub_key: from_hex(
                        "029e03a901b85534ff1e92c43c74431f7ce72046060fcf7a95c37e148f78c77255",
                    ).unwrap(),
                    short_channel_id: 72623859790382856,
                    fee_base_msat: 1,
                    fee_proportional_millionths: 20,
                    cltv_expiry_delta: 3,
                },
                ExtraHop {
                    pub_key: from_hex(
                        "039e03a901b85534ff1e92c43c74431f7ce72046060fcf7a95c37e148f78c77255",
                    ).unwrap(),
                    short_channel_id: 217304205466536202,
                    fee_base_msat: 2,
                    fee_proportional_millionths: 30,
                    cltv_expiry_delta: 4,
                },
            ],
        };

        let u5_routing_info_tag = vec![
            3u8, 5, 4, 0, 10, 15, 0, 7, 10, 8, 1, 23, 1, 10, 19, 9, 31, 24, 30, 18, 11, 2, 3, 24,
            29, 2, 3, 3, 29, 30, 14, 14, 8, 2, 6, 0, 24, 7, 28, 30, 30, 20, 21, 24, 13, 31, 1, 9,
            3, 27, 24, 24, 29, 25, 5, 10, 0, 8, 2, 0, 12, 2, 0, 10, 1, 16, 7, 1, 0, 0, 0, 0, 0, 0,
            1, 0, 0, 0, 0, 0, 5, 0, 0, 0, 12, 1, 25, 28, 0, 29, 9, 0, 6, 28, 5, 10, 13, 7, 31, 3,
            26, 9, 12, 8, 15, 3, 20, 8, 12, 15, 23, 25, 25, 25, 0, 8, 24, 3, 0, 31, 19, 27, 26, 18,
            23, 1, 23, 28, 5, 4, 15, 15, 3, 3, 23, 4, 21, 8, 3, 0, 16, 2, 16, 12, 1, 24, 8, 1, 4,
            5, 0, 0, 0, 0, 0, 0, 8, 0, 0, 0, 0, 0, 30, 0, 0, 2, 0,
        ];
        assert!(routing_info.to_vec_u5().unwrap().eq(&u5_routing_info_tag))
    }

    #[test]
    fn tag_parse_test() {
        // PaymentHashTag(0001020304050607080900010203040506070809000102030405060708090102),
        let u5_payment_hash_tag = vec![
            1u8, 1, 20, 0, 0, 0, 16, 4, 0, 24, 4, 0, 20, 3, 0, 14, 2, 0, 9, 0, 0, 0, 16, 4, 0, 24,
            4, 0, 20, 3, 0, 14, 2, 0, 9, 0, 0, 0, 16, 4, 0, 24, 4, 0, 20, 3, 0, 14, 2, 0, 9, 0, 4,
            1, 0,
        ];
        // ExpiryTag(60))
        let u5_expiry_tag = vec![6u8, 0, 2, 1, 28];
        // DescriptionTag(Please consider supporting this project))
        let u5_description_tag = vec![
            13u8, 1, 31, 10, 1, 22, 6, 10, 24, 11, 19, 12, 20, 16, 6, 6, 27, 27, 14, 14, 13, 20,
            22, 8, 25, 11, 18, 4, 1, 25, 23, 10, 28, 3, 16, 13, 29, 25, 7, 8, 26, 11, 14, 12, 28,
            16, 7, 8, 26, 3, 9, 14, 12, 16, 7, 0, 28, 19, 15, 13, 9, 18, 22, 6, 29, 0,
        ];
        // DescriptionHashTag(3925b6f67e2c340036ed12093dd44e0368df1b6ea26c53dbe4811f58fd5db8c1))
        let u5_description_hash_tag = vec![
            23u8, 1, 20, 7, 4, 18, 27, 13, 29, 19, 30, 5, 16, 26, 0, 0, 13, 23, 13, 2, 8, 4, 19,
            27, 21, 2, 14, 0, 13, 20, 13, 30, 6, 27, 14, 20, 9, 22, 5, 7, 22, 31, 4, 16, 4, 15, 21,
            17, 31, 10, 29, 23, 3, 0, 16,
        ];
        // FallbackAddressTag(17,3172b5654f6683c8fb146959d347ce303cae4ca7)
        let u5_fallback_address_tag = vec![
            9u8, 1, 1, 17, 6, 5, 25, 11, 10, 25, 10, 15, 12, 26, 1, 28, 17, 30, 24, 20, 13, 5, 12,
            29, 6, 17, 30, 14, 6, 0, 30, 10, 28, 19, 5, 7,
        ];
        // RoutingInfoTag(List(ExtraHop(029e03a901b85534ff1e92c43c74431f7ce72046060fcf7a95c37e148f78c77255,72623859790382856,1,20,3),ExtraHop(039e03a901b85534ff1e92c43c74431f7ce72046060fcf7a95c37e148f78c77255,217304205466536202,2,30,4)
        let u5_routing_info_tag = vec![
            3u8, 5, 4, 0, 10, 15, 0, 7, 10, 8, 1, 23, 1, 10, 19, 9, 31, 24, 30, 18, 11, 2, 3, 24,
            29, 2, 3, 3, 29, 30, 14, 14, 8, 2, 6, 0, 24, 7, 28, 30, 30, 20, 21, 24, 13, 31, 1, 9,
            3, 27, 24, 24, 29, 25, 5, 10, 0, 8, 2, 0, 12, 2, 0, 10, 1, 16, 7, 1, 0, 0, 0, 0, 0, 0,
            1, 0, 0, 0, 0, 0, 5, 0, 0, 0, 12, 1, 25, 28, 0, 29, 9, 0, 6, 28, 5, 10, 13, 7, 31, 3,
            26, 9, 12, 8, 15, 3, 20, 8, 12, 15, 23, 25, 25, 25, 0, 8, 24, 3, 0, 31, 19, 27, 26, 18,
            23, 1, 23, 28, 5, 4, 15, 15, 3, 3, 23, 4, 21, 8, 3, 0, 16, 2, 16, 12, 1, 24, 8, 1, 4,
            5, 0, 0, 0, 0, 0, 0, 8, 0, 0, 0, 0, 0, 30, 0, 0, 2, 0,
        ];
        // MinFinalCltvExpiryTag(12)
        let u5_min_final_cltv_expiry_tag = vec![24u8, 0, 1, 12];

        assert_eq!(
            Tag::parse(&u5_payment_hash_tag).unwrap(),
            Tag::PaymentHash {
                hash: ::utils::from_hex(
                    "0001020304050607080900010203040506070809000102030405060708090102"
                ).unwrap(),
            }
        );
        assert_eq!(
            Tag::parse(&u5_expiry_tag).unwrap(),
            Tag::Expiry { seconds: 60 }
        );
        assert_eq!(
            Tag::parse(&u5_description_tag).unwrap(),
            Tag::Description {
                description: "Please consider supporting this project".to_owned(),
            }
        );
        assert_eq!(
            Tag::parse(&u5_description_hash_tag).unwrap(),
            Tag::DescriptionHash {
                hash: ::utils::from_hex(
                    "3925b6f67e2c340036ed12093dd44e0368df1b6ea26c53dbe4811f58fd5db8c1"
                ).unwrap(),
            }
        );
        assert_eq!(
            Tag::parse(&u5_fallback_address_tag).unwrap(),
            Tag::FallbackAddress {
                version: 17,
                hash: ::utils::from_hex("3172b5654f6683c8fb146959d347ce303cae4ca7").unwrap(),
            }
        );
        assert_eq!(
            Tag::parse(&u5_routing_info_tag).unwrap(),
            Tag::RoutingInfo {
                path: vec![
                    ExtraHop {
                        pub_key: ::utils::from_hex(
                            "029e03a901b85534ff1e92c43c74431f7ce72046060fcf7a95c37e148f78c77255",
                        ).unwrap(),
                        short_channel_id: 72623859790382856,
                        fee_base_msat: 1,
                        fee_proportional_millionths: 20,
                        cltv_expiry_delta: 3,
                    },
                    ExtraHop {
                        pub_key: ::utils::from_hex(
                            "039e03a901b85534ff1e92c43c74431f7ce72046060fcf7a95c37e148f78c77255",
                        ).unwrap(),
                        short_channel_id: 217304205466536202,
                        fee_base_msat: 2,
                        fee_proportional_millionths: 30,
                        cltv_expiry_delta: 4,
                    },
                ],
            }
        );
        assert_eq!(
            Tag::parse(&u5_min_final_cltv_expiry_tag).unwrap(),
            Tag::MinFinalCltvExpiry { blocks: 12 }
        )
    }

    #[test]
    fn encode_decode_amount_test() {
        let test: HashMap<&str, f64> = hashmap!(
        "10p" => 10f64 / Unit::value('p'),
        "1n" => 1000f64 / Unit::value('p'),
        "1200p" => 1200f64 / Unit::value('p'),
        "123u" => 123f64 / Unit::value('u'),
        "123m" => 123f64 / 1000f64,
        "3" => 3f64
    );

        for (k, v) in test {
            assert_eq!(k, encode_amount(v));
            assert_eq!(v, decode_amount(&encode_amount(v)).unwrap());
        }
    }

    #[test]
    fn timestamp_test() {
        let data: Vec<U5> = vec![1, 12, 18, 31, 28, 25, 2];
        let timestamp = 1496314658;

        assert_eq!(Timestamp::decode(&data), timestamp);
        assert!(data.eq(&Timestamp::encode(timestamp)));
    }
}
