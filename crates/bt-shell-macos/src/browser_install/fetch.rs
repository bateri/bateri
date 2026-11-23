//! Fetching a file over `NSURLSession`: the system's own HTTP stack, with the system's proxy,
//! certificates and App Transport Security, and no crate. Both calls block the calling thread
//! (never the main one) until the file is here, and can be called off with a flag.
//!
//! Progress is read from the task (`countOfBytesReceived`), not from a delegate, so there is no
//! class to declare: the completion handler is the one block.

use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{RecvTimeoutError, channel};
use std::time::Duration;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_foundation::{
    NSData, NSError, NSHTTPURLResponse, NSString, NSURL, NSURLResponse, NSURLSession,
    NSURLSessionConfiguration, NSURLSessionTask,
};

/// How long the server may be silent before a request is given up on.
const IDLE: f64 = 30.0;
/// How long a whole fetch may take at the most (the archive is a few hundred megabytes).
const WHOLE: f64 = 30.0 * 60.0;

/// What a fetch tells while it runs: bytes received, and the size when the server said it.
pub type Progress<'a> = &'a (dyn Fn(u64, Option<u64>) + Sync);

fn session() -> Retained<NSURLSession> {
    let configuration = NSURLSessionConfiguration::ephemeralSessionConfiguration();
    configuration.setTimeoutIntervalForRequest(IDLE);
    configuration.setTimeoutIntervalForResource(WHOLE);
    NSURLSession::sessionWithConfiguration(&configuration)
}

fn url_of(url: &str) -> io::Result<Retained<NSURL>> {
    NSURL::URLWithString(&NSString::from_str(url))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, format!("{url} is not a URL")))
}

fn describe(error: *mut NSError) -> String {
    // SAFETY: the completion handler's error is an `NSError` or null; it lives for the call.
    unsafe { error.as_ref() }.map_or_else(
        || "the request failed".to_owned(),
        |error| error.localizedDescription().to_string(),
    )
}

/// `NSURLErrorFileDoesNotExist`: what a `file:` address that names nothing fails with.
const FILE_DOES_NOT_EXIST: isize = -1100;

/// A request that failed without an HTTP answer, as an error: a `file:` address that names nothing
/// is `ErrorKind::NotFound` as a `404` is (a release folder that has no notice in it has nothing to
/// offer), and any other failure is of the moment.
fn failed(error: *mut NSError) -> io::Error {
    // SAFETY: the completion handler's error is an `NSError` or null; it lives for the call.
    let missing = unsafe { error.as_ref() }.is_some_and(|e| e.code() == FILE_DOES_NOT_EXIST);
    let kind = if missing {
        io::ErrorKind::NotFound
    } else {
        io::ErrorKind::Other
    };
    io::Error::new(kind, describe(error))
}

/// `Some(status)` when the response is an HTTP one that is not a success.
fn http_failure(response: *mut NSURLResponse) -> Option<isize> {
    // SAFETY: the response is an `NSURLResponse` or null; it lives for the call.
    let response = unsafe { response.as_ref() }?;
    let http: &AnyObject = response.as_ref();
    let http = http.downcast_ref::<NSHTTPURLResponse>()?;
    let status = http.statusCode();
    (!(200..300).contains(&status)).then_some(status)
}

/// What a server's refusal is as an error: a file that is not there (`404`, `410`) is
/// `ErrorKind::NotFound`, which is an answer (there is nothing at that address), and any other
/// status is a failure of the moment.
fn answered(status: isize) -> io::Error {
    let kind = if matches!(status, 404 | 410) {
        io::ErrorKind::NotFound
    } else {
        io::ErrorKind::Other
    };
    io::Error::new(kind, format!("the server answered {status}"))
}

/// The bytes of `url`, at most `limit` of them (a notice, a list of sums).
pub fn bytes(url: &str, limit: usize) -> io::Result<Vec<u8>> {
    let url = url_of(url)?;
    let (done, result) = channel::<io::Result<Vec<u8>>>();
    let block = RcBlock::new(
        move |data: *mut NSData, response: *mut NSURLResponse, error: *mut NSError| {
            let outcome = if let Some(status) = http_failure(response) {
                Err(answered(status))
            } else if data.is_null() {
                Err(failed(error))
            } else {
                // SAFETY: `data` is the handler's `NSData`, non-null here, valid for the call.
                Ok(unsafe { &*data }.to_vec())
            };
            let _ = done.send(outcome);
        },
    );
    let session = session();
    // SAFETY: the block takes the three pointers the method documents.
    let task = unsafe { session.dataTaskWithURL_completionHandler(&url, &block) };
    task.resume();
    let outcome = result
        .recv()
        .map_err(|_| io::Error::other("the request ended without an answer"))?;
    session.finishTasksAndInvalidate();
    let body = outcome?;
    if body.len() > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("the answer is longer than {limit} bytes"),
        ));
    }
    Ok(body)
}

/// The file at `url` into `destination` (a path of a folder only this user can enter: the
/// caller's staging). `progress` hears of it every moment; `cancel` set ends it with
/// `ErrorKind::Interrupted`.
pub fn file(
    url: &str,
    destination: &Path,
    progress: Progress<'_>,
    cancel: &AtomicBool,
) -> io::Result<()> {
    let url = url_of(url)?;
    let (done, result) = channel::<io::Result<()>>();
    let target = destination.to_owned();
    let block = RcBlock::new(
        move |location: *mut NSURL, response: *mut NSURLResponse, error: *mut NSError| {
            let outcome = if let Some(status) = http_failure(response) {
                Err(answered(status))
            } else {
                // SAFETY: `location` is the handler's file URL (non-null on success), valid for the call.
                match unsafe { location.as_ref() }.and_then(|url| url.path()) {
                    // The file is deleted when this block returns: it is moved before that, and
                    // copied where the temporary folder is on another volume.
                    Some(path) => {
                        let from = std::path::PathBuf::from(path.to_string());
                        std::fs::rename(&from, &target)
                            .or_else(|_| std::fs::copy(&from, &target).map(drop))
                    }
                    None => Err(failed(error)),
                }
            };
            let _ = done.send(outcome);
        },
    );
    let session = session();
    // SAFETY: the block takes the three pointers the method documents.
    let task = unsafe { session.downloadTaskWithURL_completionHandler(&url, &block) };
    task.resume();
    let outcome = loop {
        match result.recv_timeout(Duration::from_millis(100)) {
            Ok(outcome) => break outcome,
            Err(RecvTimeoutError::Timeout) => {
                if cancel.load(Ordering::Acquire) {
                    task.cancel();
                    session.invalidateAndCancel();
                    return Err(io::ErrorKind::Interrupted.into());
                }
                report(&task, progress);
            }
            Err(RecvTimeoutError::Disconnected) => {
                break Err(io::Error::other("the request ended without an answer"));
            }
        }
    };
    session.finishTasksAndInvalidate();
    outcome?;
    if let Ok(meta) = std::fs::metadata(destination) {
        progress(meta.len(), Some(meta.len()));
    }
    Ok(())
}

fn report(task: &NSURLSessionTask, progress: Progress<'_>) {
    let received = u64::try_from(task.countOfBytesReceived()).unwrap_or(0);
    let expected = u64::try_from(task.countOfBytesExpectedToReceive())
        .ok()
        .filter(|&size| size > 0);
    progress(received, expected);
}
