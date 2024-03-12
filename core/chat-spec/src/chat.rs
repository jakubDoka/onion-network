use {
    super::Nonce,
    crate::{BlockNumber, ChatError, Identity, Proof},
    component_utils::{arrayvec::ArrayString, Codec, Reminder},
    std::{iter, ops::Range, time::SystemTime},
};

pub const CHAT_NAME_CAP: usize = 32;

pub type Rank = u32;

#[derive(Codec, Default, Clone, Copy, Debug)]
pub struct Member {
    pub action: Nonce,
    pub permissions: Permissions,
    pub rank: Rank,
    pub action_cooldown_ms: u32,
    #[codec(skip)]
    pub frozen_until: u64,
}

impl Member {
    pub fn best() -> Member {
        Member {
            action: 0,
            permissions: Permissions::all(),
            rank: 0,
            action_cooldown_ms: 0,
            frozen_until: 0,
        }
    }

    pub fn allocate_action(&mut self, permission: Permissions) -> Result<(), ChatError> {
        let current_ms =
            SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_millis() as u64;

        if self.frozen_until > current_ms {
            return Err(ChatError::RateLimited(self.frozen_until - current_ms));
        }
        self.frozen_until = current_ms + self.action_cooldown_ms as u64;

        self.permissions.contains(permission).then_some(()).ok_or(ChatError::NoPermission)
    }
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
    pub struct Permissions: u32 {
        const SEND = 1 << 0;
        const INVITE = 1 << 1;
        const KICK = 1 << 2;
        const RATE_LIMIT = 1 << 3;
    }
}

impl<'a> Codec<'a> for Permissions {
    fn encode(&self, buffer: &mut impl component_utils::Buffer) -> Option<()> {
        self.bits().encode(buffer)?;
        Some(())
    }

    fn decode(buffer: &mut &'a [u8]) -> Option<Self> {
        u32::decode(buffer).and_then(Self::from_bits)
    }
}

impl Member {
    pub fn combine(list: &mut [Member]) -> Member {
        assert!(!list.is_empty());

        fn most_common<T: Ord>(list: &mut [Member], f: impl Fn(&Member) -> T) -> T {
            list.sort_by_cached_key(&f);
            f(&list.chunk_by(|a, b| f(a) == f(b)).max_by_key(|chunk| chunk.len()).unwrap()[0])
        }

        fn median<T: Ord>(list: &mut [Member], f: impl Fn(&Member) -> T) -> T {
            list.sort_by_cached_key(&f);
            f(&list[list.len() / 2])
        }

        Member {
            action: list.iter().map(|m| m.action).max().unwrap(),
            permissions: most_common(list, |m| m.permissions),
            rank: most_common(list, |m| m.rank),
            action_cooldown_ms: most_common(list, |m| m.action_cooldown_ms),
            frozen_until: median(list, |m| m.frozen_until),
        }
    }
}

#[derive(Clone, Copy, Codec)]
pub struct Message<'a> {
    pub identity: Identity,
    pub nonce: Nonce,
    pub content: Reminder<'a>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Codec)]
pub struct Cursor {
    pub block: BlockNumber,
    pub offset: usize,
}

impl Cursor {
    pub const INIT: Self = Self { block: BlockNumber::MAX, offset: 0 };
}

pub type ChatName = ArrayString<CHAT_NAME_CAP>;
pub type RawChatName = [u8; CHAT_NAME_CAP];

#[derive(Codec)]
pub enum ChatEvent<'a> {
    Message(ChatName, Proof<Reminder<'a>>),
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
