//! Real sockets verify reconnect, replay limits, deadlines and cancellation.
use super::*;
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

enum Response {
    Stall(bool),
    Reply(&'static str),
}
type Requests = Arc<Mutex<Vec<String>>>;

fn server(
    responses: Vec<Response>,
    ready: Option<mpsc::Sender<usize>>,
) -> (String, Requests, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!(
        "http://{}/youtubei/v1/player",
        listener.local_addr().unwrap()
    );
    let requests = Arc::new(Mutex::new(Vec::new()));
    let seen = requests.clone();
    let task = thread::spawn(move || {
        let end = Instant::now() + Duration::from_secs(5);
        for (index, response) in responses.into_iter().enumerate() {
            let mut socket: TcpStream = loop {
                match listener.accept() {
                    Ok((socket, _)) => break socket,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < end, "Expected connection was not received");
                        thread::sleep(Duration::from_millis(2));
                    }
                    Err(e) => panic!("Loopback accept failed: {e}"),
                }
            };
            // Windows accepted sockets inherit the listener's nonblocking mode.
            socket.set_nonblocking(false).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut reader = BufReader::new(socket.try_clone().unwrap());
            let mut request = String::new();
            let mut length = 0;
            loop {
                let mut line = String::new();
                assert_ne!(reader.read_line(&mut line).unwrap(), 0);
                if line == "\r\n" {
                    break;
                }
                if let Some(n) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = n.trim().parse::<usize>().unwrap();
                    assert!(length <= 1024);
                }
                request.push_str(&line);
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            request.push_str(std::str::from_utf8(&body).unwrap());
            seen.lock().unwrap().push(request);
            if let Response::Stall(true) = response {
                socket.write_all(b"HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 0-3/4\r\nContent-Length: 4\r\n\r\nxx").unwrap();
            }
            if let Some(ready) = &ready {
                ready.send(index).unwrap();
            }
            match response {
                Response::Stall(_) => {
                    // Aborted attempt must close its socket, including a partial
                    // body, before a new connection supplies a replacement.
                    assert_eq!(reader.read(&mut [0; 1]).unwrap(), 0);
                }
                Response::Reply(reply) => {
                    socket.write_all(reply.as_bytes()).unwrap();
                }
            }
        }
    });
    (url, requests, task)
}
const GOOD: &str = "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 0-3/4\r\nContent-Length: 4\r\nConnection: close\r\n\r\nGOOD";
fn range() -> Expected {
    Expected::Range {
        start: 0,
        end: 3,
        total: 4,
    }
}
fn request(transport: &Transport, url: &str) -> Request {
    transport
        .client
        .get(url)
        .header("Range", "bytes=0-3")
        .build()
        .unwrap()
}

#[test]
fn stalled_headers_and_partial_bodies_reconnect_without_publishing_partial_bytes() {
    let transport = Transport::new().unwrap();
    for body in [false, true] {
        let (url, _, task) = server(vec![Response::Stall(body)], None);
        assert_eq!(
            transport.bytes(
                request(&transport, &url),
                range(),
                Lane::Media,
                Duration::from_millis(150),
                || false
            ),
            Err(Error::Timeout)
        );
        task.join().unwrap();

        let (url, seen, task) = server(vec![Response::Stall(body), Response::Reply(GOOD)], None);
        let started = Instant::now();
        assert_eq!(
            transport
                .media_bytes(
                    request(&transport, &url),
                    range(),
                    Duration::from_secs(2),
                    || false
                )
                .unwrap(),
            b"GOOD"
        );
        assert!(started.elapsed() < Duration::from_secs(2));
        task.join().unwrap();
        let requests = seen.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0], requests[1]);
        assert!(
            requests[1]
                .to_ascii_lowercase()
                .contains("range: bytes=0-3")
        );
    }
}

#[test]
fn repeated_stalls_have_one_reconnect_and_one_shared_absolute_deadline() {
    let transport = Transport::new().unwrap();
    let (url, seen, task) = server(vec![Response::Stall(false), Response::Stall(true)], None);
    let started = Instant::now();
    assert_eq!(
        transport.media_bytes(
            request(&transport, &url),
            range(),
            Duration::from_millis(1700),
            || false
        ),
        Err(Error::Timeout)
    );
    assert!(started.elapsed() < Duration::from_millis(2200));
    task.join().unwrap();
    assert_eq!(seen.lock().unwrap().len(), 2);
}

#[test]
fn cancellation_aborts_the_reconnect_and_releases_media_capacity() {
    let transport = Transport::new().unwrap();
    let (ready, wait) = mpsc::channel();
    let (url, seen, task) = server(
        vec![Response::Stall(false), Response::Stall(true)],
        Some(ready),
    );
    let flag = Arc::new(AtomicBool::new(false));
    let cancel = flag.clone();
    let trigger = thread::spawn(move || {
        assert_eq!(wait.recv_timeout(Duration::from_secs(2)).unwrap(), 0);
        assert_eq!(wait.recv_timeout(Duration::from_secs(2)).unwrap(), 1);
        cancel.store(true, Ordering::Release);
    });
    let started = Instant::now();
    assert_eq!(
        transport.media_bytes(
            request(&transport, &url),
            range(),
            Duration::from_secs(3),
            || flag.load(Ordering::Acquire)
        ),
        Err(Error::Cancelled)
    );
    assert!(started.elapsed() < Duration::from_millis(1600));
    trigger.join().unwrap();
    task.join().unwrap();
    assert_eq!(seen.lock().unwrap().len(), 2);
    let deadline = Instant::now() + Duration::from_secs(1);
    while transport.media.available_permits() != 4 {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn http_and_range_rejections_are_not_replayed() {
    let transport = Transport::new().unwrap();
    for (reply, error) in [
        (
            "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            Error::Http(403),
        ),
        (
            "HTTP/1.1 429 Too Many Requests\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            Error::Http(429),
        ),
        (
            "HTTP/1.1 503 Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            Error::Http(503),
        ),
        (
            "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 1-4/5\r\nContent-Length: 4\r\nConnection: close\r\n\r\nBAD!",
            Error::InvalidRange,
        ),
    ] {
        let (url, seen, task) = server(vec![Response::Reply(reply)], None);
        assert_eq!(
            transport.media_bytes(
                request(&transport, &url),
                range(),
                Duration::from_secs(2),
                || false
            ),
            Err(error)
        );
        task.join().unwrap();
        assert_eq!(seen.lock().unwrap().len(), 1);
    }
}

#[test]
fn unsigned_resolution_replays_the_same_body_but_credentials_disable_replay() {
    let transport = Transport::new().unwrap();
    let (url, seen, task) = server(
        vec![
            Response::Stall(false),
            Response::Reply("HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}"),
        ],
        None,
    );
    let request = transport
        .client
        .post(url)
        .body("{\"public\":true}")
        .build()
        .unwrap();
    assert_eq!(
        transport
            .media_bytes(
                request,
                Expected::Bytes(100),
                Duration::from_secs(2),
                || false
            )
            .unwrap(),
        b"{}"
    );
    task.join().unwrap();
    let requests = seen.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0], requests[1]);
    drop(requests);
    for header in ["Authorization", "Cookie"] {
        let (url, seen, task) = server(vec![Response::Stall(false)], None);
        let request = transport
            .client
            .post(url)
            .header(header, "fixture-only")
            .body("{}")
            .build()
            .unwrap();
        assert_eq!(
            transport.media_bytes(
                request,
                Expected::Bytes(100),
                Duration::from_millis(150),
                || false
            ),
            Err(Error::Timeout)
        );
        task.join().unwrap();
        assert_eq!(seen.lock().unwrap().len(), 1);
    }
}
