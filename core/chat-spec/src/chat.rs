use {
    super::Nonce,
    crate::{BlockNumber, Identity, Proof},
    component_utils::{arrayvec::ArrayString, Codec, Reminder},
    std::{iter, ops::Range},
};

pub const CHAT_NAME_CAP: usize = 32;

#[derive(Codec, Default, Clone)]
pub struct Member {
    pub action: Nonce,
}

#[derive(Clone, Copy, Codec)]
pub struct Message<'a> {
    pub identiy: Identity,
    pub nonce: Nonce,
    pub content: Reminder<'a>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Codec)]
pub struct Cursor {
    pub block: BlockNumber,
    pub offset: usize,
}

impl Cursor {
    pub const INIT: Self = Self { block: u64::MAX, offset: 0 };
}

pub type ChatName = ArrayString<CHAT_NAME_CAP>;
pub type RawChatName = [u8; CHAT_NAME_CAP];

#[derive(Codec)]
pub enum ChatEvent<'a> {
    Message(Proof<ChatName>, Reminder<'a>),
}

#[derive(Codec)]
pub struct ChatChecksums {
    pub size: usize,
    pub user_count: usize,
    pub message_count: usize,
}

pub fn retain_messages_in_vec(buffer: &mut Vec<u8>, predicate: impl FnMut(&mut [u8]) -> bool) {
    let len = retain_messages(buffer, predicate).len();
    buffer.drain(..buffer.len() - len);
}

/// moves all kept messages to the end of the slice and returns the kept region,
/// data at the begginig of the `buffer` is arbitrary, invalid codec is stripped
pub fn retain_messages(
    buffer: &mut [u8],
    mut predicate: impl FnMut(&mut [u8]) -> bool,
) -> &mut [u8] {
    fn move_mem(hole_end: *mut u8, cursor: *mut u8, write_cursor: &mut *mut u8, len: usize) {
        if hole_end == cursor {
            return;
        }

        let write_len = hole_end as usize - cursor as usize - len;
        if hole_end != *write_cursor {
            unsafe {
                std::ptr::copy(hole_end.sub(write_len), write_cursor.sub(write_len), write_len);
            };
        }

        *write_cursor = unsafe { write_cursor.sub(write_len) };
    }

    let Range { start, end } = buffer.as_mut_ptr_range();
    let [mut write_cursor, mut cursor, mut hole_end] = [end; 3];

    loop {
        if (cursor as usize - start as usize) < 2 {
            break;
        }

        let len = unsafe { u16::from_be_bytes(*cursor.sub(2).cast::<[u8; 2]>()) };
        let len = len as usize;

        if (cursor as usize - start as usize) < len + 2 {
            break;
        }

        cursor = unsafe { cursor.sub(len + 2) };
        let slice = unsafe { std::slice::from_raw_parts_mut(cursor, len) };
        if predicate(slice) {
            continue;
        }

        move_mem(hole_end, cursor, &mut write_cursor, len + 2);

        hole_end = cursor;
    }

    move_mem(hole_end, cursor, &mut write_cursor, 0);

    unsafe { std::slice::from_mut_ptr_range(write_cursor..end) }
}

pub fn unpack_messages(mut buffer: &mut [u8]) -> impl Iterator<Item = &mut [u8]> {
    iter::from_fn(move || {
        let len = buffer.take_mut(buffer.len().wrapping_sub(2)..)?;
        let len = u16::from_be_bytes(len.try_into().unwrap());
        buffer.take_mut(buffer.len().wrapping_sub(len as usize)..)
    })
}

pub fn unpack_messages_ref(mut buffer: &[u8]) -> impl Iterator<Item = &[u8]> {
    iter::from_fn(move || {
        let len = buffer.take(buffer.len().wrapping_sub(2)..)?;
        let len = u16::from_be_bytes(len.try_into().unwrap());
        buffer.take(buffer.len().wrapping_sub(len as usize)..)
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn retain_messages() {
        let input = [
            &[],
            &[0][..],
            &[0, 0],
            &[0, 0, 0, 0],
            &[0, 1, 0, 2, 0, 0],
            &[0, 0, 1, 0, 0, 4, 0, 0, 0, 2],
            &[0, 0, 1, 0, 0, 4, 0, 0, 0, 2, 1, 0, 1],
            &[1, 0, 1, 1, 0, 1, 0, 0, 1, 0, 1],
            &[1, 0, 0],
            &[0, 0, 0, 3],
            &[0, 0, 20],
        ];

        let output = [
            &[][..],
            &[],
            &[0, 0],
            &[0, 0, 0, 0],
            &[0, 0],
            &[0, 0, 0, 2],
            &[0, 0, 0, 2],
            &[0, 0],
            &[0, 0],
            &[],
            &[],
        ];

        for (input, output) in input.iter().zip(output.iter()) {
            let mut owned_intput = input.to_vec();
            let output = output.to_vec();
            let real_out =
                crate::retain_messages(&mut owned_intput, |bts| bts.iter().all(|b| *b == 0));
            assert_eq!(real_out, output.as_slice(), "input: {input:?}");
        }
    }
}
