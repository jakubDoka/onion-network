use codec::Codec;

type Result<T, E = ClientError> = std::result::Result<T, E>;

#[derive(Codec)]
pub enum ClientError {
    Foo,
}

pub async fn allocate_file((): ()) -> Result<()> {
    todo!()
}

pub async fn delete_file((): ()) -> Result<()> {
    todo!()
}

pub async fn allocate_bandwidth((): ()) -> Result<()> {
    todo!()
}
