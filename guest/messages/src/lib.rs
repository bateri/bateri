//! `bateri-view-messages`: what a program and its view say to each other.
//!
//! A program that shows a view and the view itself (a module in the terminal's sandbox, and one
//! day a page in a web view) talk with six messages, each a JSON object with one member named
//! for what it is:
//!
//! | member      | from the | meaning |
//! |-------------|----------|---------|
//! | `call`      | view     | asks the program to do something and answer once |
//! | `response`  | program  | the answer to a `call`: a `result`, or an `error` |
//! | `subscribe` | view     | asks to hear a `topic` as it changes, until it is cancelled |
//! | `push`      | program  | a value of a subscription |
//! | `cancel`    | view     | gives up a `call` that is still out or ends a subscription |
//! | `progress`  | program  | how far a `call` that takes a while has got |
//!
//! The terminal carries the objects between the two and **does not read them**: the encoding
//! has nothing of the sandbox in it, so the view of any other kind speaks the same messages.
//! The program and the view each read what the other says as untrusted — [`Message::parse`]
//! checks the shape and nothing more.
//!
//! An `id` belongs to the side that asked: a view's `call` and `subscribe` are numbered by the
//! view, and the `response`, `push` and `progress` that answer carry that number. Two requests
//! of a view do not share one.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The most bytes one message may have: what the terminal carries between a program and its
/// view (a message of the stream's protocol), less room for its own framing.
pub const MAX_BYTES: usize = 64 << 10;

/// One message.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Message {
    Call(Call),
    Response(Response),
    Subscribe(Subscribe),
    Push(Push),
    Cancel(Cancel),
    Progress(Progress),
}

/// The view asks the program to do something.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Call {
    pub id: u64,
    pub method: String,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub params: Value,
}

/// The program's answer to a [`Call`]. A result is never `null`: a call with nothing to say
/// answers `{}`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<Error>,
}

/// Why a call failed: a short name a view can act on, and words for the person.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Error {
    pub code: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub message: String,
}

/// The view asks to hear `topic` until it cancels.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Subscribe {
    pub id: u64,
    pub topic: String,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub params: Value,
}

/// A value of the subscription `id`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Push {
    pub id: u64,
    pub value: Value,
}

/// The view gives up the call or the subscription `id`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Cancel {
    pub id: u64,
}

/// How far the call `id` has got.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Progress {
    pub id: u64,
    pub done: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<f64>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
}

/// Why a text is not a message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refused {
    /// Over [`MAX_BYTES`].
    TooLong,
    /// Not JSON, or not one of the six, or a member missing or of the wrong kind.
    Malformed,
    /// A `response` with both a `result` and an `error`, or neither.
    Ambiguous,
    /// An `id` of 0: numbers start at 1.
    NoId,
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::TooLong => "the message is over the size one may have",
            Self::Malformed => "the message is not one of the six",
            Self::Ambiguous => "a response has a result or an error, not both and not neither",
            Self::NoId => "an id is 1 or more",
        })
    }
}

impl std::error::Error for Refused {}

impl Message {
    /// Reads `text`, which anyone could have written.
    ///
    /// # Errors
    ///
    /// Says why it is not a message ([`Refused`]).
    pub fn parse(text: &str) -> Result<Self, Refused> {
        if text.len() > MAX_BYTES {
            return Err(Refused::TooLong);
        }
        let message: Message = serde_json::from_str(text).map_err(|_| Refused::Malformed)?;
        if message.id() == 0 {
            return Err(Refused::NoId);
        }
        if let Message::Response(response) = &message
            && response.result.is_some() == response.error.is_some()
        {
            return Err(Refused::Ambiguous);
        }
        Ok(message)
    }

    /// The text of the message: compact JSON, in one line.
    pub fn text(&self) -> String {
        // A message is made of strings, numbers and values already parsed: it always serialises.
        serde_json::to_string(self).unwrap_or_default()
    }

    /// The number the message belongs to.
    pub fn id(&self) -> u64 {
        match self {
            Message::Call(m) => m.id,
            Message::Response(m) => m.id,
            Message::Subscribe(m) => m.id,
            Message::Push(m) => m.id,
            Message::Cancel(m) => m.id,
            Message::Progress(m) => m.id,
        }
    }

    /// A [`Call`].
    pub fn call(id: u64, method: &str, params: Value) -> Self {
        Self::Call(Call {
            id,
            method: method.to_owned(),
            params,
        })
    }

    /// A [`Response`] with a result.
    pub fn answer(id: u64, result: Value) -> Self {
        Self::Response(Response {
            id,
            result: Some(result),
            error: None,
        })
    }

    /// A [`Response`] that failed.
    pub fn failure(id: u64, code: &str, message: &str) -> Self {
        Self::Response(Response {
            id,
            result: None,
            error: Some(Error {
                code: code.to_owned(),
                message: message.to_owned(),
            }),
        })
    }

    /// A [`Subscribe`].
    pub fn subscribe(id: u64, topic: &str, params: Value) -> Self {
        Self::Subscribe(Subscribe {
            id,
            topic: topic.to_owned(),
            params,
        })
    }

    /// A [`Push`].
    pub fn push(id: u64, value: Value) -> Self {
        Self::Push(Push { id, value })
    }

    /// A [`Cancel`].
    pub fn cancel(id: u64) -> Self {
        Self::Cancel(Cancel { id })
    }

    /// A [`Progress`].
    pub fn progress(id: u64, done: f64, total: Option<f64>, note: &str) -> Self {
        Self::Progress(Progress {
            id,
            done,
            total,
            note: note.to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn round_trip(message: &Message) {
        let text = message.text();
        assert!(!text.contains('\n'), "{text}");
        assert_eq!(Message::parse(&text).as_ref(), Ok(message), "{text}");
    }

    #[test]
    fn each_of_the_six_is_one_object_with_one_member_and_reads_back() {
        let all = [
            Message::call(1, "stop", json!({ "name": "api" })),
            Message::answer(1, json!({ "ok": true })),
            Message::failure(1, "not-found", "no such container"),
            Message::subscribe(2, "stats", Value::Null),
            Message::push(2, json!({ "readings": [] })),
            Message::cancel(2),
            Message::progress(3, 4.0, Some(10.0), "layer 4 of 10"),
            Message::progress(3, 4.0, None, ""),
        ];
        for message in &all {
            round_trip(message);
            let object: Value = serde_json::from_str(&message.text()).unwrap();
            assert_eq!(object.as_object().unwrap().len(), 1, "{object}");
        }
    }

    #[test]
    fn the_wire_is_what_this_file_says_and_nothing_of_the_sandbox() {
        assert_eq!(
            Message::subscribe(2, "stats", Value::Null).text(),
            r#"{"subscribe":{"id":2,"topic":"stats"}}"#
        );
        assert_eq!(
            Message::call(1, "stop", json!({ "name": "api" })).text(),
            r#"{"call":{"id":1,"method":"stop","params":{"name":"api"}}}"#
        );
        assert_eq!(
            Message::failure(1, "busy", "").text(),
            r#"{"response":{"id":1,"error":{"code":"busy"}}}"#
        );
        assert_eq!(Message::cancel(7).text(), r#"{"cancel":{"id":7}}"#);
    }

    #[test]
    fn what_is_not_a_message_is_told_why() {
        let parse = |text: &str| Message::parse(text);
        assert_eq!(parse("not json"), Err(Refused::Malformed));
        assert_eq!(parse("[1]"), Err(Refused::Malformed));
        assert_eq!(parse(r#"{"hello":{"id":1}}"#), Err(Refused::Malformed));
        assert_eq!(
            parse(r#"{"call":{"id":"1","method":"x"}}"#),
            Err(Refused::Malformed)
        );
        assert_eq!(
            parse(r#"{"call":{"id":0,"method":"x"}}"#),
            Err(Refused::NoId)
        );
        assert_eq!(
            parse(r#"{"call":{"id":-1,"method":"x"}}"#),
            Err(Refused::Malformed)
        );
        assert_eq!(parse(r#"{"response":{"id":1}}"#), Err(Refused::Ambiguous));
        assert_eq!(
            parse(r#"{"response":{"id":1,"result":1,"error":{"code":"x"}}}"#),
            Err(Refused::Ambiguous)
        );
        let long = format!(
            r#"{{"push":{{"id":1,"value":"{}"}}}}"#,
            "a".repeat(MAX_BYTES)
        );
        assert_eq!(parse(&long), Err(Refused::TooLong));
    }

    #[test]
    fn a_member_nobody_asked_for_is_ignored_and_a_result_is_never_null() {
        let call = Message::parse(r#"{"call":{"id":5,"method":"x","extra":true}}"#).unwrap();
        assert_eq!(call, Message::call(5, "x", Value::Null));
        // A call that has nothing to say answers `{}`: `null` is no result at all.
        assert_eq!(
            Message::parse(r#"{"response":{"id":5,"result":{}}}"#),
            Ok(Message::answer(5, json!({})))
        );
        assert_eq!(
            Message::parse(r#"{"response":{"id":5,"result":null}}"#),
            Err(Refused::Ambiguous)
        );
    }
}
