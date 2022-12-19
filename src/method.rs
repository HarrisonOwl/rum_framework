use strum_macros::{Display, EnumString};


#[derive(Debug, Display, EnumString)]
pub enum MethodType{
    GET,
    POST,
    PUT,
    DELETE,
    CONNECT,
    OPTIONS,
    TRACE,
    PATCH
}