use crate::ProtocolError;

pub const DART_MAX_DATA_BYTES: usize = 524_288;
pub const DART_USHM_BYTES: u32 = 1_048_576;
pub const DART_MAX_P_INDEX: u32 = 65_535;
pub const DART_WRITE_ACK: u8 = 0xba;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum DartCommand {
    GetUshmChar = 0x10,
    SetUshmChar = 0x11,
    GetUshmInt = 0x40,
    SetUshmInt = 0x41,
    GetUshmFloat = 0xf0,
    SetUshmFloat = 0xf1,
    GetUshmDouble = 0x80,
    SetUshmDouble = 0x81,
    GetPVariable = 0x90,
    SetPVariable = 0x91,
}

impl TryFrom<u32> for DartCommand {
    type Error = ProtocolError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            0x10 => Ok(Self::GetUshmChar),
            0x11 => Ok(Self::SetUshmChar),
            0x40 => Ok(Self::GetUshmInt),
            0x41 => Ok(Self::SetUshmInt),
            0xf0 => Ok(Self::GetUshmFloat),
            0xf1 => Ok(Self::SetUshmFloat),
            0x80 => Ok(Self::GetUshmDouble),
            0x81 => Ok(Self::SetUshmDouble),
            0x90 => Ok(Self::GetPVariable),
            0x91 => Ok(Self::SetPVariable),
            _ => Err(ProtocolError::Dart(format!(
                "unrecognized command 0x{value:x}"
            ))),
        }
    }
}

impl DartCommand {
    pub fn is_write(self) -> bool {
        matches!(
            self,
            Self::SetUshmChar
                | Self::SetUshmInt
                | Self::SetUshmFloat
                | Self::SetUshmDouble
                | Self::SetPVariable
        )
    }

    pub fn element_size(self) -> usize {
        match self {
            Self::GetUshmChar | Self::SetUshmChar => 1,
            Self::GetUshmInt
            | Self::SetUshmInt
            | Self::GetUshmFloat
            | Self::SetUshmFloat => 4,
            Self::GetUshmDouble
            | Self::SetUshmDouble
            | Self::GetPVariable
            | Self::SetPVariable => 8,
        }
    }

    fn is_p_variable(self) -> bool {
        matches!(self, Self::GetPVariable | Self::SetPVariable)
    }

    pub fn native_to_wire(self, mut data: Vec<u8>) -> Vec<u8> {
        if cfg!(target_endian = "little") {
            swap_elements(&mut data, self.element_size());
        }
        data
    }

    pub fn wire_to_native(self, mut data: Vec<u8>) -> Vec<u8> {
        if cfg!(target_endian = "little") {
            swap_elements(&mut data, self.element_size());
        }
        data
    }
}

fn swap_elements(data: &mut [u8], width: usize) {
    for element in data.chunks_exact_mut(width) {
        element.reverse();
    }
}

#[derive(Debug, Clone)]
pub struct DartTransfer {
    command: DartCommand,
    address: u32,
    length: usize,
    data: Vec<u8>,
}

impl DartTransfer {
    pub fn get(
        command: DartCommand,
        address: u32,
        length: usize,
    ) -> Result<Self, ProtocolError> {
        if command.is_write() {
            return Err(ProtocolError::Dart(
                "a write command cannot be used for Get".into(),
            ));
        }
        let transfer = Self {
            command,
            address,
            length,
            data: Vec::new(),
        };
        transfer.validate_length(length)?;
        Ok(transfer)
    }

    pub fn set(
        command: DartCommand,
        address: u32,
        data: Vec<u8>,
    ) -> Result<Self, ProtocolError> {
        if !command.is_write() {
            return Err(ProtocolError::Dart(
                "a read command cannot be used for Set".into(),
            ));
        }
        let length = data.len();
        let transfer = Self {
            command,
            address,
            length,
            data,
        };
        transfer.validate_length(length)?;
        Ok(transfer)
    }

    pub fn command(&self) -> DartCommand {
        self.command
    }

    pub fn address(&self) -> u32 {
        self.address
    }

    pub fn data(&self) -> &[u8] {
        &self.data
    }

    pub fn len(&self) -> usize {
        self.length
    }

    pub fn into_wire_data(self) -> Vec<u8> {
        self.command.native_to_wire(self.data)
    }

    pub fn wire_header(&self) -> [u8; 12] {
        let mut header = [0; 12];
        header[0..4].copy_from_slice(&(self.command as u32).to_be_bytes());
        header[4..8].copy_from_slice(&self.address.to_be_bytes());
        header[8..12].copy_from_slice(&(self.len() as u32).to_be_bytes());
        header
    }

    fn validate_length(&self, length: usize) -> Result<(), ProtocolError> {
        if length == 0 || length > DART_MAX_DATA_BYTES {
            return Err(ProtocolError::Dart(format!(
                "data length must be 1..={DART_MAX_DATA_BYTES} bytes"
            )));
        }
        let width = self.command.element_size();
        if length % width != 0 {
            return Err(ProtocolError::Dart(format!(
                "data length {length} is not aligned to {width}-byte elements"
            )));
        }
        if self.command.is_p_variable() {
            let count = u32::try_from(length / width)
                .map_err(|_| ProtocolError::Dart("P-variable count overflow".into()))?;
            let end = self
                .address
                .checked_add(count)
                .ok_or_else(|| ProtocolError::Dart("P-variable range overflow".into()))?;
            if self.address > DART_MAX_P_INDEX || end > DART_MAX_P_INDEX + 1 {
                return Err(ProtocolError::Dart(
                    "P-variable range must be within P0..P65535".into(),
                ));
            }
        } else {
            let length = u32::try_from(length)
                .map_err(|_| ProtocolError::Dart("USHM length overflow".into()))?;
            let end = self
                .address
                .checked_add(length)
                .ok_or_else(|| ProtocolError::Dart("USHM range overflow".into()))?;
            if end > DART_USHM_BYTES {
                return Err(ProtocolError::Dart(
                    "USHM address plus data length must not exceed 1048576".into(),
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_values_and_big_endian_header_match_the_document() {
        let transfer = DartTransfer::get(DartCommand::GetPVariable, 65_535, 8).unwrap();
        assert_eq!(
            transfer.wire_header(),
            [0, 0, 0, 0x90, 0, 0, 0xff, 0xff, 0, 0, 0, 8]
        );
        assert_eq!(DartCommand::try_from(0x10).unwrap(), DartCommand::GetUshmChar);
        assert!(DartCommand::try_from(0x20).is_err());
        assert!(DartCommand::try_from(0x21).is_err());
        assert!(DartCommand::try_from(0x92).is_err());
    }

    #[test]
    fn every_element_type_requires_aligned_nonempty_bounded_data() {
        for (command, width) in [
            (DartCommand::GetUshmChar, 1),
            (DartCommand::GetUshmInt, 4),
            (DartCommand::GetUshmFloat, 4),
            (DartCommand::GetUshmDouble, 8),
            (DartCommand::GetPVariable, 8),
        ] {
            assert!(DartTransfer::get(command, 0, width).is_ok());
            assert!(DartTransfer::get(command, 0, 0).is_err());
            if width > 1 {
                assert!(DartTransfer::get(command, 0, width + 1).is_err());
            }
            assert!(DartTransfer::get(command, 0, DART_MAX_DATA_BYTES + 1).is_err());
        }
    }

    #[test]
    fn ushm_and_p_variable_ranges_include_the_exact_last_element() {
        assert!(DartTransfer::get(DartCommand::GetUshmDouble, DART_USHM_BYTES - 8, 8).is_ok());
        assert!(DartTransfer::get(DartCommand::GetUshmDouble, DART_USHM_BYTES - 7, 8).is_err());
        assert!(DartTransfer::get(DartCommand::GetPVariable, 65_535, 8).is_ok());
        assert!(DartTransfer::get(DartCommand::GetPVariable, 65_535, 16).is_err());
        assert!(DartTransfer::get(DartCommand::GetPVariable, 65_536, 8).is_err());
    }

    #[test]
    fn set_moves_the_buffer_and_rejects_direction_or_length_mismatch() {
        let data = vec![1; 16];
        let pointer = data.as_ptr();
        let transfer = DartTransfer::set(DartCommand::SetPVariable, 1, data).unwrap();
        assert_eq!(transfer.data().as_ptr(), pointer);
        assert!(DartTransfer::set(DartCommand::GetPVariable, 0, vec![0; 8]).is_err());
        assert!(DartTransfer::get(DartCommand::SetPVariable, 0, 8).is_err());
    }

    #[test]
    fn big_endian_conversion_reverses_each_element_without_crossing_boundaries() {
        let mut data = [1, 2, 3, 4, 5, 6, 7, 8];
        swap_elements(&mut data, 4);
        assert_eq!(data, [4, 3, 2, 1, 8, 7, 6, 5]);
    }
}
