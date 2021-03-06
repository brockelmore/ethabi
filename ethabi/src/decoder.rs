//! ABI decoder.

use util::slice_data;
use {Token, ErrorKind, Error, ResultExt, ParamType};

struct DecodeResult {
	token: Token,
	new_offset: usize,
	tail_consumed: usize,
}

struct BytesTaken {
	bytes: Vec<u8>,
	new_offset: usize,
}

fn as_u32(slice: &[u8; 32]) -> Result<u32, Error> {
	if !slice[..28].iter().all(|x| *x == 0) {
		return Err(ErrorKind::InvalidData.into());
	}

	let result = ((slice[28] as u32) << 24) +
		((slice[29] as u32) << 16) +
		((slice[30] as u32) << 8) +
		(slice[31] as u32);

	Ok(result)
}

fn as_bool(slice: &[u8; 32]) -> Result<bool, Error> {
	if !slice[..31].iter().all(|x| *x == 0) {
		return Err(ErrorKind::InvalidData.into());
	}

	Ok(slice[31] == 1)
}

/// Decodes ABI compliant vector of bytes into vector of tokens described by types param.
pub fn decode(types: &[ParamType], data: &[u8]) -> Result<Vec<Token>, Error> {
    let is_empty_bytes_valid_encoding = types.iter().all(|t| t.is_empty_bytes_valid_encoding());
    if !is_empty_bytes_valid_encoding && data.is_empty() {
        bail!("please ensure the contract and method you're calling exist! failed to decode empty bytes. if you're using jsonrpc this is likely due to jsonrpc returning `0x` in case contract or method don't exist");
    }
	let slices = slice_data(data)?;
	let mut tokens = vec![];
	let mut offset = 0;
	for param in types {
		let res = decode_param(param, &slices, offset).chain_err(|| format!("Cannot decode {}", param))?;
		offset = res.new_offset;
		tokens.push(res.token);
	}
	Ok(tokens)
}

fn peek(slices: &[[u8; 32]], position: usize) -> Result<&[u8; 32], Error> {
	slices.get(position).ok_or_else(|| ErrorKind::InvalidData.into())
}

fn take_bytes(slices: &[[u8; 32]], position: usize, len: usize) -> Result<BytesTaken, Error> {
	let slices_len = (len + 31) / 32;

	let mut bytes_slices = vec![];
	for i in 0..slices_len {
		let slice = try!(peek(slices, position + i));
		bytes_slices.push(slice);
	}

	let bytes = bytes_slices.into_iter()
		.flat_map(|slice| slice.to_vec())
		.take(len)
		.collect();

	let taken = BytesTaken {
		bytes,
		new_offset: position + slices_len,
	};

	Ok(taken)
}

fn decode_param(param: &ParamType, slices: &[[u8; 32]], offset: usize) -> Result<DecodeResult, Error> {
	match *param {
		ParamType::Address => {
			let slice = try!(peek(slices, offset));
			let mut address = [0u8; 20];
			address.copy_from_slice(&slice[12..]);

			let result = DecodeResult {
				token: Token::Address(address.into()),
				new_offset: offset + 1,
				tail_consumed: 0,
			};

			Ok(result)
		},
		ParamType::Int(_) => {
			let slice = try!(peek(slices, offset));

			let result = DecodeResult {
				token: Token::Int(slice.clone().into()),
				new_offset: offset + 1,
				tail_consumed: 0,
			};

			Ok(result)
		},
		ParamType::Uint(_) => {
			let slice = try!(peek(slices, offset));

			let result = DecodeResult {
				token: Token::Uint(slice.clone().into()),
				new_offset: offset + 1,
				tail_consumed: 0,
			};

			Ok(result)
		},
		ParamType::Bool => {
			let slice = try!(peek(slices, offset));

			let b = try!(as_bool(slice));

			let result = DecodeResult {
				token: Token::Bool(b),
				new_offset: offset + 1,
				tail_consumed: 0,
			};

			Ok(result)
		},
		ParamType::FixedBytes(len) => {
			let taken = try!(take_bytes(slices, offset, len));

			let result = DecodeResult {
				token: Token::FixedBytes(taken.bytes),
				new_offset: taken.new_offset,
				tail_consumed: 0,
			};

			Ok(result)
		},
		ParamType::Bytes => {
			let offset_slice = try!(peek(slices, offset));
			let len_offset = (try!(as_u32(offset_slice)) / 32) as usize;

			let len_slice = try!(peek(slices, len_offset));
			let len = try!(as_u32(len_slice)) as usize;

			let taken = try!(take_bytes(slices, len_offset + 1, len));

			let result = DecodeResult {
				token: Token::Bytes(taken.bytes),
				new_offset: offset + 1,
				tail_consumed: len % 32 + 1,
			};

			Ok(result)
		},
		ParamType::String => {
			let offset_slice = try!(peek(slices, offset));
			let len_offset = (try!(as_u32(offset_slice)) / 32) as usize;

			let len_slice = try!(peek(slices, len_offset));
			let len = try!(as_u32(len_slice)) as usize;

			let taken = try!(take_bytes(slices, len_offset + 1, len));

			let result = DecodeResult {
				token: Token::String(try!(String::from_utf8(taken.bytes))),
				new_offset: offset + 1,
				tail_consumed: 0,
			};

			Ok(result)
		},
		ParamType::Array(ref t) => {
			let offset_slice = try!(peek(slices, offset));
			let len_offset = (try!(as_u32(offset_slice)) / 32) as usize;

			let len_slice = try!(peek(slices, len_offset));
			let len = try!(as_u32(len_slice)) as usize;

			let sub_slices = &slices[len_offset + 1..];
			let mut tokens = vec![];
			let mut new_offset = 0;

			for _ in 0..len {
				let res = try!(decode_param(t, &sub_slices, new_offset));
				new_offset = res.new_offset;
				tokens.push(res.token);
			}

			let result = DecodeResult {
				token: Token::Array(tokens),
				new_offset: offset + 1,
				tail_consumed: 0,
			};

			Ok(result)
		},
		ParamType::FixedArray(ref t, len) => {
			let mut tokens = vec![];
			let mut new_offset = offset;
			for _ in 0..len {
				let res = try!(decode_param(t, &slices, new_offset));
				new_offset = res.new_offset;
				tokens.push(res.token);
			}

			let result = DecodeResult {
				token: Token::FixedArray(tokens),
				new_offset,
				tail_consumed: 0,
			};

			Ok(result)
		}
		ParamType::Tuple(ref params) => {
			let mut tokens = vec![];
			let mut new_offset;
			let mut tail_consumed = 0; // storing dynamic type's tail size.

			if param.is_dynamic() {
				let offset_slice = try!(peek(slices, offset));
				let len_offset = (try!(as_u32(offset_slice)) / 32) as usize;
				let tail_slices = &slices[len_offset..];

				let mut dynamic_offset = 0;

				new_offset = offset + 1;
				for param in params.iter() {
					if param.is_dynamic() {
						let res = decode_param(param, tail_slices, dynamic_offset)?;
						dynamic_offset += 1;
						tokens.push(res.token);
						tail_consumed += res.tail_consumed;
					} else {
						let res = decode_param(param, tail_slices, dynamic_offset)?;
						dynamic_offset = res.new_offset;
						tokens.push(res.token);
					}
				}
			} else {
				new_offset = offset;
				for param in params.iter() {
					let res = decode_param(param, &slices, new_offset)?;
					new_offset = res.new_offset;
					tokens.push(res.token);
				}
			}

			let result = DecodeResult {
				token: Token::Tuple(tokens),
				new_offset,
				tail_consumed,
			};

			Ok(result)
		},
	}
}

#[cfg(test)]
mod tests {
	use {decode, Token, ParamType};

	#[test]
	fn decode_address() {
		let encoded = hex!("0000000000000000000000001111111111111111111111111111111111111111");
		let address = Token::Address([0x11u8; 20].into());
		let expected = vec![address];
		let decoded = decode(&[ParamType::Address], &encoded).unwrap();
		assert_eq!(decoded, expected);
	}

	#[test]
	fn decode_two_address() {
		let encoded = hex!("
					   0000000000000000000000001111111111111111111111111111111111111111
					   0000000000000000000000002222222222222222222222222222222222222222
					");
		let address1 = Token::Address([0x11u8; 20].into());
		let address2 = Token::Address([0x22u8; 20].into());
		let expected = vec![address1, address2];
		let decoded = decode(&[ParamType::Address, ParamType::Address], &encoded).unwrap();
		assert_eq!(decoded, expected);
	}

	#[test]
	fn decode_fixed_array_of_addresses() {
		let encoded = hex!("
					   0000000000000000000000001111111111111111111111111111111111111111
					   0000000000000000000000002222222222222222222222222222222222222222
					");
		let address1 = Token::Address([0x11u8; 20].into());
		let address2 = Token::Address([0x22u8; 20].into());
		let expected = vec![Token::FixedArray(vec![address1, address2])];
		let decoded = decode(&[ParamType::FixedArray(Box::new(ParamType::Address), 2)], &encoded).unwrap();
		assert_eq!(decoded, expected);
	}

	#[test]
	fn decode_uint() {
		let encoded = hex!("1111111111111111111111111111111111111111111111111111111111111111");
		let uint = Token::Uint([0x11u8; 32].into());
		let expected = vec![uint];
		let decoded = decode(&[ParamType::Uint(32)], &encoded).unwrap();
		assert_eq!(decoded, expected);
	}

	#[test]
	fn decode_int() {
		let encoded = hex!("1111111111111111111111111111111111111111111111111111111111111111");
		let int = Token::Int([0x11u8; 32].into());
		let expected = vec![int];
		let decoded = decode(&[ParamType::Int(32)], &encoded).unwrap();
		assert_eq!(decoded, expected);
	}

	#[test]
	fn decode_dynamic_array_of_addresses() {
		let encoded = hex!("
			0000000000000000000000000000000000000000000000000000000000000020
			0000000000000000000000000000000000000000000000000000000000000002
			0000000000000000000000001111111111111111111111111111111111111111
			0000000000000000000000002222222222222222222222222222222222222222
		");
		let address1 = Token::Address([0x11u8; 20].into());
		let address2 = Token::Address([0x22u8; 20].into());
		let addresses = Token::Array(vec![address1, address2]);
		let expected = vec![addresses];
		let decoded = decode(&[ParamType::Array(Box::new(ParamType::Address))], &encoded).unwrap();
		assert_eq!(decoded, expected);
	}

	#[test]
	fn decode_dynamic_array_of_fixed_arrays() {
		let encoded = hex!("
			0000000000000000000000000000000000000000000000000000000000000020
			0000000000000000000000000000000000000000000000000000000000000002
			0000000000000000000000001111111111111111111111111111111111111111
			0000000000000000000000002222222222222222222222222222222222222222
			0000000000000000000000003333333333333333333333333333333333333333
			0000000000000000000000004444444444444444444444444444444444444444
		");
		let address1 = Token::Address([0x11u8; 20].into());
		let address2 = Token::Address([0x22u8; 20].into());
		let address3 = Token::Address([0x33u8; 20].into());
		let address4 = Token::Address([0x44u8; 20].into());
		let array0 = Token::FixedArray(vec![address1, address2]);
		let array1 = Token::FixedArray(vec![address3, address4]);
		let dynamic = Token::Array(vec![array0, array1]);
		let expected = vec![dynamic];
		let decoded = decode(&[
			ParamType::Array(Box::new(
				ParamType::FixedArray(Box::new(ParamType::Address), 2)
			))
		], &encoded).unwrap();
		assert_eq!(decoded, expected);
	}

	#[test]
	fn decode_dynamic_array_of_dynamic_arrays() {
		let encoded  = hex!("
			0000000000000000000000000000000000000000000000000000000000000020
			0000000000000000000000000000000000000000000000000000000000000002
			0000000000000000000000000000000000000000000000000000000000000040
			0000000000000000000000000000000000000000000000000000000000000080
			0000000000000000000000000000000000000000000000000000000000000001
			0000000000000000000000001111111111111111111111111111111111111111
			0000000000000000000000000000000000000000000000000000000000000001
			0000000000000000000000002222222222222222222222222222222222222222
		");

		let address1 = Token::Address([0x11u8; 20].into());
		let address2 = Token::Address([0x22u8; 20].into());
		let array0 = Token::Array(vec![address1]);
		let array1 = Token::Array(vec![address2]);
		let dynamic = Token::Array(vec![array0, array1]);
		let expected = vec![dynamic];
		let decoded = decode(&[
			ParamType::Array(Box::new(
				ParamType::Array(Box::new(ParamType::Address))
			))
		], &encoded).unwrap();
		assert_eq!(decoded, expected);
	}

	#[test]
	fn decode_dynamic_array_of_dynamic_arrays2() {
		let encoded = hex!("
			0000000000000000000000000000000000000000000000000000000000000020
			0000000000000000000000000000000000000000000000000000000000000002
			0000000000000000000000000000000000000000000000000000000000000040
			00000000000000000000000000000000000000000000000000000000000000a0
			0000000000000000000000000000000000000000000000000000000000000002
			0000000000000000000000001111111111111111111111111111111111111111
			0000000000000000000000002222222222222222222222222222222222222222
			0000000000000000000000000000000000000000000000000000000000000002
			0000000000000000000000003333333333333333333333333333333333333333
			0000000000000000000000004444444444444444444444444444444444444444
		");

		let address1 = Token::Address([0x11u8; 20].into());
		let address2 = Token::Address([0x22u8; 20].into());
		let address3 = Token::Address([0x33u8; 20].into());
		let address4 = Token::Address([0x44u8; 20].into());
		let array0 = Token::Array(vec![address1, address2]);
		let array1 = Token::Array(vec![address3, address4]);
		let dynamic = Token::Array(vec![array0, array1]);
		let expected = vec![dynamic];
		let decoded = decode(&[
			ParamType::Array(Box::new(
				ParamType::Array(Box::new(ParamType::Address))
			))
		], &encoded).unwrap();
		assert_eq!(decoded, expected);
	}

	#[test]
	fn decode_fixed_array_fixed_arrays() {
		let encoded = hex!("
			0000000000000000000000001111111111111111111111111111111111111111
			0000000000000000000000002222222222222222222222222222222222222222
			0000000000000000000000003333333333333333333333333333333333333333
			0000000000000000000000004444444444444444444444444444444444444444
		");
		let address1 = Token::Address([0x11u8; 20].into());
		let address2 = Token::Address([0x22u8; 20].into());
		let address3 = Token::Address([0x33u8; 20].into());
		let address4 = Token::Address([0x44u8; 20].into());
		let array0 = Token::FixedArray(vec![address1, address2]);
		let array1 = Token::FixedArray(vec![address3, address4]);
		let fixed = Token::FixedArray(vec![array0, array1]);
		let expected = vec![fixed];

		let decoded = decode(&[
			ParamType::FixedArray(
				Box::new(ParamType::FixedArray(Box::new(ParamType::Address), 2)),
				2
			)
		], &encoded).unwrap();

		assert_eq!(decoded, expected);
	}

	#[test]
	fn decode_fixed_array_of_dynamic_array_of_addresses() {
		let encoded = hex!("
			0000000000000000000000000000000000000000000000000000000000000040
			00000000000000000000000000000000000000000000000000000000000000a0
			0000000000000000000000000000000000000000000000000000000000000002
			0000000000000000000000001111111111111111111111111111111111111111
			0000000000000000000000002222222222222222222222222222222222222222
			0000000000000000000000000000000000000000000000000000000000000002
			0000000000000000000000003333333333333333333333333333333333333333
			0000000000000000000000004444444444444444444444444444444444444444
		");
		let address1 = Token::Address([0x11u8; 20].into());
		let address2 = Token::Address([0x22u8; 20].into());
		let address3 = Token::Address([0x33u8; 20].into());
		let address4 = Token::Address([0x44u8; 20].into());
		let array0 = Token::Array(vec![address1, address2]);
		let array1 = Token::Array(vec![address3, address4]);
		let fixed = Token::FixedArray(vec![array0, array1]);
		let expected = vec![fixed];

		let decoded = decode(&[
			ParamType::FixedArray(
				Box::new(ParamType::Array(Box::new(ParamType::Address))),
				2
			)
		], &encoded).unwrap();

		assert_eq!(decoded, expected);
	}

	#[test]
	fn decode_fixed_bytes() {
		let encoded = hex!("1234000000000000000000000000000000000000000000000000000000000000");
		let bytes = Token::FixedBytes(vec![0x12, 0x34]);
		let expected = vec![bytes];
		let decoded = decode(&[ParamType::FixedBytes(2)], &encoded).unwrap();
		assert_eq!(decoded, expected);
	}

	#[test]
	fn decode_bytes() {
		let encoded = hex!("
			0000000000000000000000000000000000000000000000000000000000000020
			0000000000000000000000000000000000000000000000000000000000000002
			1234000000000000000000000000000000000000000000000000000000000000
		");
		let bytes = Token::Bytes(vec![0x12, 0x34]);
		let expected = vec![bytes];
		let decoded = decode(&[ParamType::Bytes], &encoded).unwrap();
		assert_eq!(decoded, expected);
	}

	#[test]
	fn decode_bytes2() {
		let encoded = hex!("
			0000000000000000000000000000000000000000000000000000000000000020
			0000000000000000000000000000000000000000000000000000000000000040
			1000000000000000000000000000000000000000000000000000000000000000
			1000000000000000000000000000000000000000000000000000000000000000
		");
		let bytes = Token::Bytes(hex!("
			1000000000000000000000000000000000000000000000000000000000000000
			1000000000000000000000000000000000000000000000000000000000000000
		").to_vec());
		let expected = vec![bytes];
		let decoded = decode(&[ParamType::Bytes], &encoded).unwrap();
		assert_eq!(decoded, expected);
	}

	#[test]
	fn decode_two_bytes() {
		let encoded = hex!("
			0000000000000000000000000000000000000000000000000000000000000040
			0000000000000000000000000000000000000000000000000000000000000080
			000000000000000000000000000000000000000000000000000000000000001f
			1000000000000000000000000000000000000000000000000000000000000200
			0000000000000000000000000000000000000000000000000000000000000020
			0010000000000000000000000000000000000000000000000000000000000002
		");
		let bytes1 = Token::Bytes(hex!("10000000000000000000000000000000000000000000000000000000000002").to_vec());
		let bytes2 = Token::Bytes(hex!("0010000000000000000000000000000000000000000000000000000000000002").to_vec());
		let expected = vec![bytes1, bytes2];
		let decoded = decode(&[ParamType::Bytes, ParamType::Bytes], &encoded).unwrap();
		assert_eq!(decoded, expected);
	}

	#[test]
	fn decode_string() {
		let encoded = hex!("
			0000000000000000000000000000000000000000000000000000000000000020
			0000000000000000000000000000000000000000000000000000000000000009
			6761766f66796f726b0000000000000000000000000000000000000000000000
		");
		let s = Token::String("gavofyork".to_owned());
		let expected = vec![s];
		let decoded = decode(&[ParamType::String], &encoded).unwrap();
		assert_eq!(decoded, expected);
	}

	#[test]
	fn decode_from_empty_byte_slice() {
		// these can NOT be decoded from empty byte slice
		assert!(decode(&[ParamType::Address], &[]).is_err());
		assert!(decode(&[ParamType::Bytes], &[]).is_err());
		assert!(decode(&[ParamType::Int(0)], &[]).is_err());
		assert!(decode(&[ParamType::Int(1)], &[]).is_err());
		assert!(decode(&[ParamType::Int(0)], &[]).is_err());
		assert!(decode(&[ParamType::Int(1)], &[]).is_err());
		assert!(decode(&[ParamType::Bool], &[]).is_err());
		assert!(decode(&[ParamType::String], &[]).is_err());
		assert!(decode(&[ParamType::Array(Box::new(ParamType::Bool))], &[]).is_err());
		assert!(decode(&[ParamType::FixedBytes(1)], &[]).is_err());
		assert!(decode(&[ParamType::FixedArray(Box::new(ParamType::Bool), 1)], &[]).is_err());

		// these are the only ones that can be decoded from empty byte slice
		assert!(decode(&[ParamType::FixedBytes(0)], &[]).is_ok());
		assert!(decode(&[ParamType::FixedArray(Box::new(ParamType::Bool), 0)], &[]).is_ok());
	}

	#[test]
	fn decode_tuple() {
		let encoded = hex!("
			0000000000000000000000001111111111111111111111111111111111111111
			000000000000000000000000000000000000000000000000000000000000250f
		");
		let address = Token::Address([0x11u8; 20].into());
		let uint = Token::Uint(9487.into());
		let expected = vec![Token::Tuple(vec![address, uint])];
		let decoded = decode(&[ParamType::Tuple(vec![ParamType::Address, ParamType::Uint(256)])], &encoded).unwrap();
		assert_eq!(decoded, expected);
	}

	#[test]
	fn decode_dynamic_tuple() {
		let encoded = hex!(
			"
			0000000000000000000000000000000000000000000000000000000000000020
			0000000000000000000000000000000000000000000000000000000000000001
			0000000000000000000000000000000000000000000000000000000000000040
			0000000000000000000000000000000000000000000000000000000000000004
			6461746100000000000000000000000000000000000000000000000000000000
			"
		);

		let uint1 = Token::Uint(1.into());
		let bytes = Token::Bytes(b"data"[..].into());
		let expected = vec![
			Token::Tuple(vec![uint1, bytes])
		];

		let decoded = decode(
			&[ParamType::Tuple(vec![ParamType::Uint(256), ParamType::Bytes])],
			&encoded
		).unwrap();
		assert_eq!(decoded, expected);
	}

	#[test]
	fn decode_dynamic_tuple2() {
		let encoded = hex!(
			"
			0000000000000000000000000000000000000000000000000000000000000040
			0000000000000000000000000000000000000000000000000000000000000001
			0000000000000000000000000000000000000000000000000000000000000001
			0000000000000000000000000000000000000000000000000000000000000040
			0000000000000000000000000000000000000000000000000000000000000004
			6461746100000000000000000000000000000000000000000000000000000000
			"
		);

		let uint1 = Token::Uint(1.into());
		let uint2 = Token::Uint(1.into());
		let bytes = Token::Bytes(b"data"[..].into());
		let expected = vec![
			Token::Tuple(vec![uint1, bytes]),
			uint2
		];

		let decoded = decode(
			&[ParamType::Tuple(vec![ParamType::Uint(256), ParamType::Bytes]), ParamType::Uint(256)],
			&encoded
		).unwrap();
		assert_eq!(decoded, expected);
	}

	#[test]
	fn decode_uint_and_dynamic_tuple() {
		let encoded = hex!(
			"
			000000000000000000000000000000000000000000000000000000000000250f
			0000000000000000000000000000000000000000000000000000000000000040
			0000000000000000000000001111111111111111111111111111111111111111
			0000000000000000000000000000000000000000000000000000000000000040
			0000000000000000000000000000000000000000000000000000000000000004
			6461746100000000000000000000000000000000000000000000000000000000
		"
		);

		let address = Token::Address([0x11u8; 20].into());
		let bytes = Token::Bytes(b"data"[..].into());
		let tuple = Token::Tuple(vec![address, bytes]);
		let uint = Token::Uint(9487.into());

		let expected = vec![
			uint,
			tuple
		];

		let decoded = decode(
			&[
				ParamType::Uint(256),
				ParamType::Tuple(vec![ParamType::Address, ParamType::Bytes])
			],
			&encoded
		).unwrap();

		assert_eq!(decoded, expected);
	}

	#[test]
	fn decode_nested_tuple() {
		let encoded = hex!(
			"
			0000000000000000000000000000000000000000000000000000000000000020
			0000000000000000000000000000000000000000000000000000000000000040
			0000000000000000000000000000000000000000000000000000000000000001
			0000000000000000000000000000000000000000000000000000000000000001
			0000000000000000000000000000000000000000000000000000000000000040
			0000000000000000000000000000000000000000000000000000000000000004
			6461746100000000000000000000000000000000000000000000000000000000
			"
		);

		let uint1 = Token::Uint(1.into());
		let uint2 = Token::Uint(1.into());
		let bytes = Token::Bytes(b"data"[..].into());
		let tuple = Token::Tuple(vec![uint1, bytes]);

		let expected = vec![
			Token::Tuple(vec![tuple, uint2])
		];

		let decoded = decode(
			&[
				ParamType::Tuple(vec![ParamType::Tuple(vec![ParamType::Uint(256), ParamType::Bytes]), ParamType::Uint(256)])
			],
			&encoded
		).unwrap();

		assert_eq!(decoded, expected);

	}

	#[test]
	fn decode_tuple_pattern1() {
		let address = Token::Address([0x11u8; 20].into());
		let bytes = Token::Bytes(vec![0x30u8, 0x31u8, 0x32u8, 0x33u8]);
		let tuple = Token::Tuple(vec![address, bytes]);
		let uint1 = Token::Uint(25.into());
		let uint2 = Token::Uint(30.into());
		let expected = vec![tuple, uint1, uint2];
		let encoded = hex!(
			"
			0000000000000000000000000000000000000000000000000000000000000060
			0000000000000000000000000000000000000000000000000000000000000019
			000000000000000000000000000000000000000000000000000000000000001e
			0000000000000000000000001111111111111111111111111111111111111111
			0000000000000000000000000000000000000000000000000000000000000040
			0000000000000000000000000000000000000000000000000000000000000004
			3031323300000000000000000000000000000000000000000000000000000000
		"
		)
		.to_vec();
		let decoded = decode(
			&[
				ParamType::Tuple(vec![ParamType::Address, ParamType::Bytes]),
				ParamType::Uint(256),
				ParamType::Uint(256)
			],
			&encoded
		).unwrap();

		assert_eq!(decoded, expected);
	}

	#[test]
	fn decode_tuple_pattern2() {
		let address = Token::Address([0x11u8; 20].into());
		let bytes = Token::Bytes(vec![0x30u8, 0x31u8, 0x32u8, 0x33u8]);
		let state_object = Token::Tuple(vec![address, bytes]);
		let state_update_range_start = Token::Uint(15.into());
		let state_update_range_end = Token::Uint(16.into());
		let state_update_range = Token::Tuple(vec![state_update_range_start, state_update_range_end]);
		let block_number = Token::Uint(25.into());
		let plasma_address = Token::Address([0x22u8; 20].into());
		let state_update = Token::Tuple(vec![state_object, state_update_range, block_number, plasma_address]);
		let range_start = Token::Uint(0.into());
		let range_end = Token::Uint(100.into());
		let range = Token::Tuple(vec![range_start, range_end]);
		let expected = vec![state_update, range];
		let encoded = hex!(
			"
			0000000000000000000000000000000000000000000000000000000000000060
			0000000000000000000000000000000000000000000000000000000000000000
			0000000000000000000000000000000000000000000000000000000000000064
			00000000000000000000000000000000000000000000000000000000000000a0
			000000000000000000000000000000000000000000000000000000000000000f
			0000000000000000000000000000000000000000000000000000000000000010
			0000000000000000000000000000000000000000000000000000000000000019
			0000000000000000000000002222222222222222222222222222222222222222
			0000000000000000000000001111111111111111111111111111111111111111
			0000000000000000000000000000000000000000000000000000000000000040
			0000000000000000000000000000000000000000000000000000000000000004
			3031323300000000000000000000000000000000000000000000000000000000
		"
		)
		.to_vec();
		let decoded = decode(
			&[
				ParamType::Tuple(vec![
					ParamType::Tuple(vec![ParamType::Address, ParamType::Bytes]),
					ParamType::Tuple(vec![ParamType::Uint(256), ParamType::Uint(256)]),
					ParamType::Uint(256),
					ParamType::Address,
				]),
				ParamType::Tuple(vec![ParamType::Uint(256), ParamType::Uint(256)])
			],
			&encoded
		).unwrap();

		assert_eq!(decoded, expected);
	}

	fn decode_dynamic_array_of_bytes() {
		let encoded = hex!("
			000000000000000000000000000000000000000000000000000000000000000c
			0000000000000000000000000000000000000000000000000000000000000040
			0000000000000000000000000000000000000000000000000000000000000002
			0000000000000000000000000000000000000000000000000000000000000040
			0000000000000000000000000000000000000000000000000000000000000080
			0000000000000000000000000000000000000000000000000000000000000002
			1231000000000000000000000000000000000000000000000000000000000000
			0000000000000000000000000000000000000000000000000000000000000002
			1232000000000000000000000000000000000000000000000000000000000000
		");
		let b1 = Token::Bytes(vec![0x12, 0x31]);
		let b2 = Token::Bytes(vec![0x12, 0x32]);
		let dynamic = Token::Array(vec![b1, b2]);
		let expected = vec![Token::Uint(12.into()), dynamic];
		let decoded = decode(&[
			ParamType::Uint(256),
			ParamType::Array(Box::new(
				ParamType::Bytes
			))
		], &encoded).unwrap();
		assert_eq!(decoded, expected);
	}
}

