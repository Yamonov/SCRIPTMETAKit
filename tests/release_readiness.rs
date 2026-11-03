#[cfg(feature = "blocking-http")]
#[test]
fn http_validator_retention_follows_network_and_memory_cache_settings() {
    use scriptmetakit::{ScanMode, ScriptMetaKitConfig, ScriptMetaKitEngine};
    use std::{
        fs,
        io::{Read, Write},
        net::TcpListener,
        thread,
        time::{Duration, Instant},
    };
    for (network_cache, memory_cache) in [(true, true), (false, true), (true, false)] {
        let directory = tempfile::tempdir().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}/metadata", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            let mut conditional_requests = Vec::new();
            for _ in 0..2 {
                let deadline = Instant::now() + Duration::from_secs(5);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error)
                            if error.kind() == std::io::ErrorKind::WouldBlock
                                && Instant::now() < deadline =>
                        {
                            thread::sleep(Duration::from_millis(1))
                        }
                        Err(error) => panic!("HTTP fixture did not receive a request: {error}"),
                    }
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut request = Vec::new();
                let mut chunk = [0; 1024];
                while !request.windows(4).any(|part| part == b"\r\n\r\n") {
                    let count = stream.read(&mut chunk).unwrap();
                    assert!(count > 0 && request.len() < 16384);
                    request.extend_from_slice(&chunk[..count]);
                }
                conditional_requests.push(
                    String::from_utf8(request)
                        .unwrap()
                        .to_ascii_lowercase()
                        .contains("if-none-match:"),
                );
                let body = "SCRIPTMETA-DIST-BEGIN\nScript-ID=cache-policy\nVersion=2.0\nSCRIPTMETA-DIST-END";
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nETag: \"version-one\"\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
            conditional_requests
        });
        fs::write(directory.path().join("sample.jsx"), format!("/*\nSCRIPTMETA-BEGIN\nScript-ID=cache-policy\nVersion=1.0\nMeta-URL={url}\nSCRIPTMETA-END\n*/")).unwrap();
        let mut config = ScriptMetaKitConfig::default();
        config.watcher.enabled = false;
        config.cache.persistent_cache = false;
        config.cache.memory_cache = memory_cache;
        config.update_check.cache_network_responses = network_cache;
        let mut engine = ScriptMetaKitEngine::new(config).unwrap();
        let scanned = engine
            .scan_root_paths([directory.path()], ScanMode::FileListAndMetadata)
            .unwrap();
        let items = &scanned.catalog_snapshot.as_ref().unwrap().file_items;
        for _ in 0..2 {
            let result = pollster::block_on(engine.check_updates_for_items(items)).unwrap();
            assert!(result.errors_by_item_id.is_empty());
        }
        assert_eq!(
            server.join().unwrap(),
            [false, network_cache && memory_cache],
            "network={network_cache}, memory={memory_cache}"
        );
    }
}
