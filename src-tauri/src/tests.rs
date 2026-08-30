use super::*;

#[test]
fn cloud_reports_require_explicit_opt_in() {
    assert!(!cloud_reports_enabled_from_config(&json!({})));
    assert!(!cloud_reports_enabled_from_config(&json!({
        "cloud_reports_enabled": false
    })));
    assert!(cloud_reports_enabled_from_config(&json!({
        "cloud_reports_enabled": true
    })));
}

#[test]
fn extension_setup_only_opens_known_chromium_pages() {
    assert_eq!(extension_browser_url("opera_gx"), Some("opera:extensions"));
    assert_eq!(extension_browser_url("opera"), Some("opera:extensions"));
    assert_eq!(extension_browser_url("chrome"), Some("chrome://extensions"));
    assert_eq!(extension_browser_url("edge"), Some("edge://extensions"));
    assert_eq!(extension_browser_url("firefox"), None);
    assert_eq!(extension_browser_url("../../cmd.exe"), None);
}

#[test]
fn extension_setup_launches_opera_without_an_internal_page_argument() {
    assert_eq!(extension_browser_launch_page("opera_gx"), Some(None));
    assert_eq!(extension_browser_launch_page("opera"), Some(None));
    assert_eq!(
        extension_browser_launch_page("chrome"),
        Some(Some("chrome://extensions"))
    );
    assert_eq!(
        extension_browser_launch_page("edge"),
        Some(Some("edge://extensions"))
    );
    assert_eq!(extension_browser_launch_page("firefox"), None);
}

#[test]
fn extension_setup_detects_program_files_opera_launchers() {
    let local = Path::new(r"C:\Users\User\AppData\Local");
    let program_files = Path::new(r"C:\Program Files");
    let program_files_x86 = Path::new(r"C:\Program Files (x86)");

    let gx = cookie_browser_executable_candidates_for_roots(
        "opera_gx",
        Some(local),
        Some(program_files),
        Some(program_files_x86),
    );
    let opera = cookie_browser_executable_candidates_for_roots(
        "opera",
        Some(local),
        Some(program_files),
        Some(program_files_x86),
    );

    assert!(gx.contains(&program_files.join("Opera GX").join("launcher.exe")));
    assert!(gx.contains(&program_files_x86.join("Opera GX").join("launcher.exe")));
    assert!(opera.contains(&program_files.join("Opera").join("launcher.exe")));
    assert!(opera.contains(&program_files_x86.join("Opera").join("launcher.exe")));
}

#[test]
fn extension_setup_launch_argument_is_explicit() {
    assert!(extension_setup_requested(&[
        "mediadrop.exe".to_string(),
        "--extension-setup".to_string(),
    ]));
    assert!(!extension_setup_requested(&[
        "mediadrop.exe".to_string(),
        "--companion".to_string(),
    ]));
}

fn push_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn push_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn sample_sidx() -> Vec<u8> {
    let mut data = Vec::new();
    push_u32(&mut data, 56);
    data.extend_from_slice(b"sidx");
    push_u32(&mut data, 0); // version + flags
    push_u32(&mut data, 1); // reference_ID
    push_u32(&mut data, 1000); // timescale
    push_u32(&mut data, 0); // earliest_presentation_time
    push_u32(&mut data, 0); // first_offset
    push_u16(&mut data, 0); // reserved
    push_u16(&mut data, 2); // reference_count
    push_u32(&mut data, 1000); // reference size
    push_u32(&mut data, 5000); // 5 seconds
    push_u32(&mut data, 0); // SAP flags
    push_u32(&mut data, 2000); // reference size
    push_u32(&mut data, 5000); // 5 seconds
    push_u32(&mut data, 0); // SAP flags
    data
}

#[test]
fn parses_sidx_references() {
    let index = parse_sidx_index(&sample_sidx()).expect("sidx should parse");

    assert_eq!(index.header_end, 56);
    assert_eq!(index.references.len(), 2);
    assert_eq!(index.references[0].byte_start, 56);
    assert_eq!(index.references[0].byte_end, 1055);
    assert_eq!(index.references[1].start_time, 5.0);
    assert_eq!(index.references[1].end_time, 10.0);
}

#[test]
fn selects_header_and_matching_sidx_ranges() {
    let index = parse_sidx_index(&sample_sidx()).expect("sidx should parse");
    let (ranges, media_range) = select_sidx_ranges(
        &index,
        ClipRange {
            start: 4.5,
            end: 6.0,
        },
    )
    .expect("ranges should be selected");

    assert_eq!(media_range.start, 0.0);
    assert_eq!(media_range.end, 10.0);
    assert_eq!(ranges.len(), 1);
    assert_eq!(ranges[0].start, 0);
    assert_eq!(ranges[0].end, 3055);
}

#[test]
fn true_quality_attempts_prefer_visionos_for_seekable_4k_streams() {
    let attempts = make_true_quality_clip_attempts("401", "4K");
    let first = attempts.first().expect("at least one True Quality attempt");

    assert_eq!(first.video_selector, "401");
    assert_eq!(
        first.extractor_args,
        Some("youtube:player_client=visionos")
    );
    assert!(attempts.iter().any(|attempt| {
        attempt.extractor_args == Some("youtube:player_client=visionos")
            && attempt.video_selector.starts_with("bestvideo[height=2160]")
    }));
}

#[test]
fn youtube_hls_clip_selector_merges_separate_video_and_audio_streams() {
    let selector = youtube_hls_clip_selector("1080p");

    assert!(selector.starts_with(
        "bestvideo[protocol*=m3u8][height<=1080]+bestaudio[protocol*=m3u8]"
    ));
    assert!(selector.contains(
        "/best[protocol*=m3u8][height<=1080][vcodec!=none][acodec!=none]"
    ));
}

#[test]
fn youtube_selected_selector_preserves_combined_formats_and_bounds_fallbacks() {
    let selector = youtube_selected_selector("22", "720p");

    assert_eq!(
        selector,
        "22[acodec!=none]/22[acodec=none]+bestaudio[ext=m4a]/22[acodec=none]+bestaudio/bestvideo[height<=720]+bestaudio[ext=m4a]/bestvideo[height<=720]+bestaudio/best[height<=720][vcodec!=none][acodec!=none]"
    );
}

#[test]
fn ytdlp_commands_ignore_external_config_and_plugins() {
    let command = ytdlp_command("yt-dlp.exe");
    let args = command
        .get_args()
        .map(|arg| arg.to_string_lossy().to_string())
        .collect::<Vec<_>>();

    assert_eq!(args, ["--ignore-config", "--no-plugin-dirs"]);
}

#[cfg(target_os = "windows")]
#[test]
fn youtube_curl_is_resolved_from_the_windows_system_directory() {
    let path = windows_system_tool_path("curl.exe").expect("Windows system curl");

    assert!(path.is_absolute());
    assert_eq!(path.file_name().and_then(OsStr::to_str), Some("curl.exe"));
    assert!(windows_system_tool_path(r"..\curl.exe").is_none());
}

#[test]
fn youtube_download_commands_keep_tls_checks_and_use_structured_progress() {
    let tools = RuntimeTools {
        yt_dlp: PathBuf::from("yt-dlp.exe"),
        aria2c: PathBuf::from("aria2c.exe"),
        ffmpeg_dir: PathBuf::from("runtime"),
    };
    let attempts = make_youtube_attempts(
        "video",
        "137",
        "1080p",
        false,
        false,
        Some(Path::new(r"C:\Windows\System32\curl.exe")),
    );

    assert!(!attempts
        .iter()
        .any(|attempt| attempt.label.to_lowercase().contains("sertifika")));

    for attempt in attempts {
        let external_downloader = attempt.external_downloader.clone();
        let command = build_download_command(
            &tools,
            Path::new("downloads"),
            "video.%(ext)s",
            "video",
            &attempt,
            None,
        );
        let args = command
            .get_args()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect::<Vec<_>>();

        assert!(!args.iter().any(|arg| arg == "--no-check-certificate"));
        assert!(args
            .windows(2)
            .any(|pair| pair == ["--concurrent-fragments", "4"]));
        assert!(args.windows(2).any(|pair| {
            pair[0] == "--progress-template"
                && pair[1].starts_with("download:__MEDIADROP_PROGRESS__")
        }));

        if external_downloader == ExternalDownloader::Aria2c {
            assert!(args
                .windows(2)
                .any(|pair| pair == ["--downloader", "dash,m3u8:native"]));
        }
    }
}

#[test]
fn youtube_curl_rescue_caps_tls_and_keeps_segment_downloads_native() {
    let tools = RuntimeTools {
        yt_dlp: PathBuf::from("yt-dlp.exe"),
        aria2c: PathBuf::from("aria2c.exe"),
        ffmpeg_dir: PathBuf::from("runtime"),
    };
    let mut attempt = download_attempt("YouTube TLS 1.2 uyumluluk modu", "137+bestaudio");
    let system_curl = PathBuf::from(r"C:\Windows\System32\curl.exe");
    attempt.external_downloader = ExternalDownloader::Curl(system_curl.clone());

    let command = build_download_command(
        &tools,
        Path::new("downloads"),
        "video.%(ext)s",
        "video",
        &attempt,
        None,
    );
    let args = command
        .get_args()
        .map(|arg| arg.to_string_lossy().to_string())
        .collect::<Vec<_>>();

    assert!(args
        .windows(2)
        .any(|pair| pair == ["--downloader", "dash,m3u8:native"]));
    assert!(args.windows(2).any(|pair| {
        pair[0] == "--downloader" && pair[1] == system_curl.to_string_lossy()
    }));
    let curl_args = args
        .windows(2)
        .find(|pair| pair[0] == "--downloader-args")
        .map(|pair| pair[1].as_str())
        .expect("curl downloader args");
    assert!(curl_args.contains("--http1.1"));
    assert!(curl_args.contains("--tls-max 1.2"));
}

#[test]
fn youtube_tls_compatibility_attempt_is_explicitly_direct_https_only() {
    let attempt = make_youtube_attempts(
        "video",
        "137",
        "1080p",
        false,
        false,
        Some(Path::new(r"C:\Windows\System32\curl.exe")),
    )
    .into_iter()
    .find(|attempt| matches!(attempt.external_downloader, ExternalDownloader::Curl(_)))
    .expect("direct HTTPS TLS 1.2 compatibility attempt");

    assert_eq!(attempt.label, "YouTube direct HTTPS / TLS 1.2 uyumluluk modu");
    assert!(!attempt.label.to_lowercase().contains("dash"));
    assert!(!attempt.label.to_lowercase().contains("hls"));
}

#[test]
fn youtube_tries_tls12_before_repeating_other_network_paths() {
    let attempts = make_youtube_attempts(
        "video",
        "137",
        "1080p",
        false,
        false,
        Some(Path::new(r"C:\Windows\System32\curl.exe")),
    );
    let tls12_index = attempts
        .iter()
        .position(|attempt| matches!(attempt.external_downloader, ExternalDownloader::Curl(_)))
        .expect("TLS 1.2 curl rescue");
    let chunk_index = attempts
        .iter()
        .position(|attempt| attempt.http_chunk_size.is_some())
        .expect("HTTP chunk rescue");

    assert!(tls12_index < chunk_index);
}

#[test]
fn youtube_network_failure_is_not_mislabeled_as_missing_format() {
    let mixed_error = "[SSL: INVALID_SESSION_ID] invalid session id\nERROR: Requested format is not available";

    assert_eq!(friendly_media_access_error("YouTube", mixed_error), None);
    assert!(user_friendly_download_error(
        true,
        &[("Stabil mod".to_string(), mixed_error.to_string())]
    )
    .contains("googlevideo CDN"));
}

#[test]
fn youtube_tls_only_failure_is_not_mislabeled_as_missing_format() {
    let tls_error = "[SSL: INVALID_SESSION_ID] invalid session id";

    assert_eq!(friendly_media_access_error("YouTube", tls_error), None);
    assert!(user_friendly_download_error(
        true,
        &[("Stabil mod".to_string(), tls_error.to_string())]
    )
    .contains("googlevideo CDN"));
}

#[test]
fn youtube_tls_diagnosis_limits_tls12_compatibility_to_direct_https() {
    let diagnosis = user_friendly_download_error(
        true,
        &[(
            "YouTube direct HTTPS / TLS 1.2 uyumluluk modu".to_string(),
            "[SSL: INVALID_SESSION_ID] invalid session id".to_string(),
        )],
    );

    assert!(diagnosis.contains("Direct HTTPS aktarımı için TLS 1.2 uyumluluk denendi"));
    assert!(diagnosis.contains("DASH/HLS veya segmentli aktarım bu yöntemle kurtarılmaz"));
}

#[test]
fn youtube_format_only_failure_remains_a_format_selection_error() {
    let format_error = "ERROR: Requested format is not available";

    assert!(friendly_media_access_error("YouTube", format_error)
        .expect("format-only error")
        .contains("uygun format bulunamadı"));
    assert!(!user_friendly_download_error(
        true,
        &[("Stabil mod".to_string(), format_error.to_string())]
    )
    .contains("googlevideo CDN"));
}

#[test]
fn cached_youtube_analysis_is_materialized_only_for_the_matching_url() {
    let url = format!(
        "https://www.youtube.com/watch?v=cache{}",
        unique_stamp()
    );
    let info_json = format!(
        r#"{{"id":"cache","webpage_url":"{url}","formats":[{{"format_id":"22","url":"https://example.test/video.mp4","ext":"mp4","protocol":"https","vcodec":"avc1.64001f","acodec":"mp4a.40.2","height":720}}]}}"#
    );

    cache_youtube_analysis(&url, &info_json).expect("valid analysis should cache");

    let mut cached_command = ytdlp_command("yt-dlp.exe");
    let artifact = add_ytdlp_media_source(&mut cached_command, &url)
        .expect("cached source should materialize")
        .expect("cached source should use a temp info json");
    let cached_args = cached_command
        .get_args()
        .map(|arg| arg.to_string_lossy().to_string())
        .collect::<Vec<_>>();

    assert_eq!(fs::read_to_string(artifact.path()).unwrap(), info_json);
    assert!(cached_args.windows(2).any(|pair| {
        pair[0] == "--load-info-json" && Path::new(&pair[1]) == artifact.path()
    }));
    assert!(!cached_args.iter().any(|arg| arg == &url));

    invalidate_youtube_analysis(&url);

    let mut live_command = ytdlp_command("yt-dlp.exe");
    assert!(add_ytdlp_media_source(&mut live_command, &url)
        .expect("live source should be accepted")
        .is_none());
    assert!(live_command
        .get_args()
        .any(|arg| arg.to_string_lossy() == url));
}

#[test]
fn youtube_analysis_cache_rejects_invalid_or_expired_entries() {
    let url = format!(
        "https://www.youtube.com/watch?v=invalid{}",
        unique_stamp()
    );

    assert!(cache_youtube_analysis(&url, "{}").is_err());
    assert!(youtube_analysis_cache_entry_is_fresh(Duration::ZERO));
    assert!(youtube_analysis_cache_entry_is_fresh(Duration::from_secs(299)));
    assert!(!youtube_analysis_cache_entry_is_fresh(Duration::from_secs(300)));
}

#[test]
fn youtube_analysis_cache_skips_live_media_and_invalidates_on_auth_change() {
    let live_url = format!("https://www.youtube.com/watch?v=live{}", unique_stamp());
    let live_json = format!(
        r#"{{"id":"live","webpage_url":"{live_url}","is_live":true,"live_status":"is_live","formats":[{{"format_id":"95","url":"https://example.test/live.m3u8","ext":"mp4","protocol":"m3u8_native","vcodec":"avc1","acodec":"mp4a","height":720}}]}}"#
    );
    cache_youtube_analysis(&live_url, &live_json).expect("live analysis should be accepted");
    assert!(
        cached_youtube_analysis(&live_url).is_none(),
        "live media must not reuse signed stream URLs"
    );

    let vod_url = format!("https://www.youtube.com/watch?v=auth{}", unique_stamp());
    let vod_json = format!(
        r#"{{"id":"vod","webpage_url":"{vod_url}","formats":[{{"format_id":"22","url":"https://example.test/video.mp4","ext":"mp4","protocol":"https","vcodec":"avc1","acodec":"mp4a","height":720}}]}}"#
    );
    cache_youtube_analysis(&vod_url, &vod_json).expect("VOD analysis should cache");
    assert!(cached_youtube_analysis(&vod_url).is_some());

    let cookies = concat!(
        "# Netscape HTTP Cookie File\n",
        ".youtube.com\tTRUE\t/\tTRUE\t0\tSAPISID\tfixture-session\n"
    );
    register_ytdlp_cookie_jar(&vod_url, "chrome", cookies).unwrap();
    assert!(
        cached_youtube_analysis(&vod_url).is_none(),
        "changing auth context must invalidate the old analysis"
    );
    prepared_ytdlp_cookies().lock().unwrap().remove(&vod_url);
}

#[test]
fn parses_structured_ytdlp_progress_without_human_output_assumptions() {
    let metrics = parse_yt_dlp_progress(
        "__MEDIADROP_PROGRESS__\tdownloading\t5242880\t10485760\tNA\t1048576",
    )
    .expect("structured progress should parse");

    assert_eq!(metrics.percent, Some(50.0));
    assert_eq!(metrics.downloaded_mb, Some(5.0));
    assert_eq!(metrics.total_mb, Some(10.0));
    assert_eq!(metrics.speed_mb, Some(1.0));
}

#[test]
fn youtube_preview_attempt_timeout_honors_one_total_budget() {
    assert_eq!(
        youtube_preview_attempt_timeout(Duration::ZERO),
        Some(Duration::from_secs(5))
    );
    assert_eq!(
        youtube_preview_attempt_timeout(Duration::from_secs(4)),
        Some(Duration::from_secs(1))
    );
    assert_eq!(
        youtube_preview_attempt_timeout(Duration::from_secs(5)),
        None
    );
}

#[test]
fn youtube_preview_selector_pairs_separate_video_with_audio() {
    let selector = youtube_preview_selector("4K");

    assert!(selector.contains(
        "bestvideo[height<=1080][ext=mp4]+bestaudio[ext=m4a]"
    ));
    assert!(selector.split('/').all(|candidate| candidate == "22"
        || candidate == "18"
        || candidate.contains("[acodec!=none]")
        || candidate.contains("+bestaudio")));
}

#[test]
fn dash_sidx_probes_fetch_only_new_bytes() {
    let video_ranges = dash_probe_ranges(false)
        .into_iter()
        .map(|range| (range.start, range.end))
        .collect::<Vec<_>>();
    let audio_ranges = dash_probe_ranges(true)
        .into_iter()
        .map(|range| (range.start, range.end))
        .collect::<Vec<_>>();

    assert_eq!(
        video_ranges,
        vec![
            (0, 2 * 1024 * 1024 - 1),
            (2 * 1024 * 1024, 4 * 1024 * 1024 - 1),
            (4 * 1024 * 1024, 8 * 1024 * 1024 - 1),
            (8 * 1024 * 1024, 16 * 1024 * 1024 - 1),
        ]
    );
    assert_eq!(
        audio_ranges,
        vec![
            (0, 1024 * 1024 - 1),
            (1024 * 1024, 2 * 1024 * 1024 - 1),
            (2 * 1024 * 1024, 4 * 1024 * 1024 - 1),
        ]
    );
}

#[test]
fn media_probe_validation_requires_audio() {
    let probe = serde_json::json!({
        "streams": [
            {
                "codec_type": "video",
                "codec_name": "h264",
                "width": 1920,
                "height": 1080,
                "duration": "10.0",
                "start_time": "0.0"
            }
        ],
        "format": { "duration": "10.0" }
    });

    let error = validate_media_probe_value(&probe, Some(1080), None)
        .expect_err("video downloads must contain audio");

    assert!(error.contains("ses stream"));
}

#[test]
fn media_probe_validation_rejects_low_resolution_and_av_drift() {
    let low_resolution = serde_json::json!({
        "streams": [
            {
                "codec_type": "video",
                "codec_name": "h264",
                "width": 1280,
                "height": 720,
                "duration": "10.0",
                "start_time": "0.0"
            },
            {
                "codec_type": "audio",
                "codec_name": "aac",
                "duration": "10.0",
                "start_time": "0.0"
            }
        ],
        "format": { "duration": "10.0" }
    });
    let drifted_audio = serde_json::json!({
        "streams": [
            {
                "codec_type": "video",
                "codec_name": "h264",
                "width": 1920,
                "height": 1080,
                "duration": "10.0",
                "start_time": "0.0"
            },
            {
                "codec_type": "audio",
                "codec_name": "aac",
                "duration": "4.0",
                "start_time": "2.0"
            }
        ],
        "format": { "duration": "10.0" }
    });

    assert!(validate_media_probe_value(&low_resolution, Some(1080), None)
        .expect_err("lower resolution must fail")
        .contains("kalitenin altında"));
    assert!(validate_media_probe_value(&drifted_audio, Some(1080), None)
        .expect_err("large A/V drift must fail")
        .contains("ses/video"));
}

#[test]
fn media_probe_validation_accepts_complete_expected_output() {
    let probe = serde_json::json!({
        "streams": [
            {
                "codec_type": "video",
                "codec_name": "vp9",
                "width": 3840,
                "height": 2160,
                "duration": "12.1",
                "start_time": "0.0"
            },
            {
                "codec_type": "audio",
                "codec_name": "opus",
                "duration": "12.0",
                "start_time": "0.02"
            }
        ],
        "format": { "duration": "12.1" }
    });

    validate_media_probe_value(&probe, Some(2160), Some(12.0))
        .expect("complete output should validate");
}

#[test]
fn media_progress_gate_throttles_updates_but_never_completion() {
    assert!(!media_progress_update_due(
        Duration::from_millis(99),
        false
    ));
    assert!(media_progress_update_due(
        Duration::from_millis(100),
        false
    ));
    assert!(media_progress_update_due(Duration::ZERO, true));
}

#[test]
fn http_range_rejects_insecure_initial_urls_before_connecting() {
    let client = http_range_client(Duration::from_secs(45)).expect("range client");
    let error = fetch_http_range(
        &client,
        "http://127.0.0.1:9/private-media",
        &DashByteRange { start: 0, end: 63 },
    )
    .expect_err("insecure range URL must be rejected");

    assert_eq!(error, "HTTP range linki guvenli degil.");
}

#[test]
fn http_content_range_validation_rejects_mismatched_or_truncated_responses() {
    let requested = DashByteRange { start: 100, end: 199 };
    let returned = validate_http_content_range("bytes 100-199/500", &requested, false)
        .expect("an exact range should validate");
    assert_eq!((returned.start, returned.end), (100, 199));

    for value in [
        "bytes 99-199/500",
        "bytes 100-200/500",
        "bytes 100-150/500",
        "items 100-199/500",
        "bytes 100-199/199",
    ] {
        assert!(
            validate_http_content_range(value, &requested, false).is_err(),
            "unexpected valid Content-Range: {value}"
        );
    }

    let eof = validate_http_content_range(
        "bytes 100-149/150",
        &requested,
        true,
    )
    .expect("a probe may end early only at the declared EOF");
    assert_eq!((eof.start, eof.end), (100, 149));
    assert!(validate_http_content_range("bytes 100-149/*", &requested, true).is_err());
}

#[test]
fn http_range_body_streams_in_chunks_and_rejects_wrong_length() {
    let payload = vec![7_u8; 192 * 1024 + 17];
    let mut output = Vec::new();
    let mut updates = Vec::new();
    let copied = stream_http_range_body(
        std::io::Cursor::new(&payload),
        &mut output,
        payload.len() as u64,
        |written| {
            updates.push(written);
            Ok(())
        },
    )
    .expect("complete response should stream");

    assert_eq!(copied, payload.len() as u64);
    assert_eq!(output, payload);
    assert!(updates.len() >= 3, "large responses must not be buffered at once");

    let mut stopped_output = Vec::new();
    let stop_error = stream_http_range_body(
        std::io::Cursor::new(&payload),
        &mut stopped_output,
        payload.len() as u64,
        |_| Err("stop".to_string()),
    )
    .expect_err("a stop request must interrupt the stream between chunks");
    assert_eq!(stop_error, "stop");
    assert!(stopped_output.len() < payload.len());

    let short_error = stream_http_range_body(
        std::io::Cursor::new(b"short"),
        &mut Vec::new(),
        10,
        |_| Ok(()),
    )
    .expect_err("truncated bodies must fail");
    assert!(short_error.contains("eksik"));

    let long_error = stream_http_range_body(
        std::io::Cursor::new(b"too long"),
        &mut Vec::new(),
        3,
        |_| Ok(()),
    )
    .expect_err("overlong bodies must fail");
    assert!(long_error.contains("fazla"));
}

#[test]
fn cached_media_is_materialized_without_network_download() {
    let dir = std::env::temp_dir().join(format!("mediadrop-cache-reuse-{}", unique_stamp()));
    fs::create_dir_all(&dir).unwrap();
    let source = dir.join("source.jpg");
    let target = dir.join("target.jpg");
    fs::write(&source, [0xff, 0xd8, 0xff, 0xe0, 1, 2, 3, 4]).unwrap();

    assert_eq!(materialize_cached_media_file(&source, &target).unwrap(), 8);
    assert_eq!(fs::read(&target).unwrap(), fs::read(&source).unwrap());

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn bounded_ytdlp_stderr_keeps_the_latest_complete_utf8_tail() {
    let mut log = String::new();
    append_bounded_text(&mut log, "first-line", 16);
    append_bounded_text(&mut log, "second-line", 16);
    assert!(log.len() <= 16);
    assert!(log.ends_with("second-line\n"));

    append_bounded_text(&mut log, "🙂🙂🙂", 10);
    assert!(log.len() <= 10);
    assert_eq!(log, "🙂🙂\n");
}

#[test]
fn merges_adjacent_byte_ranges() {
    let ranges = merge_dash_byte_ranges(vec![
        DashByteRange { start: 20, end: 30 },
        DashByteRange { start: 0, end: 9 },
        DashByteRange { start: 10, end: 19 },
        DashByteRange { start: 40, end: 41 },
    ]);

    assert_eq!(ranges.len(), 2);
    assert_eq!(ranges[0].start, 0);
    assert_eq!(ranges[0].end, 30);
    assert_eq!(ranges[1].start, 40);
    assert_eq!(ranges[1].end, 41);
}

#[test]
fn process_line_reader_keeps_reading_after_non_utf8_bytes() {
    let bytes = b"first\nbad \xDD line\nlast\r\n".to_vec();
    let mut lines = Vec::new();

    read_process_lines_lossy(std::io::Cursor::new(bytes), |line| lines.push(line));

    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0], "first");
    assert!(lines[1].starts_with("bad "));
    assert_eq!(lines[2], "last");
}

#[test]
fn cleanup_recognizes_only_mediadrop_owned_temp_outputs() {
    assert!(is_mediadrop_internal_temp_artifact(Path::new(
        "mediadrop-temp-abc123.mp4"
    )));
    assert!(is_mediadrop_internal_temp_artifact(Path::new(
        "mediadrop-temp-abc123.mp4.part"
    )));
    assert!(is_mediadrop_internal_temp_artifact(Path::new(
        "mediadrop-temp-abc123.f140.m4a"
    )));
    assert!(is_mediadrop_internal_temp_artifact(Path::new(
        "mediadrop-temp-true-quality-abc123.mp4"
    )));
    assert!(!is_mediadrop_internal_temp_artifact(Path::new(
        "Finished MediaDrop Video.mp4"
    )));
    assert!(is_mediadrop_internal_temp_artifact(Path::new(
        "mediadrop-temp-abc123.mp4.part"
    )));
    assert!(is_mediadrop_internal_temp_artifact(Path::new(
        "md-hls-123-mediadrop-temp-abc123.mp4"
    )));
    assert!(!is_mediadrop_internal_temp_artifact(Path::new(
        "other-app-download.part"
    )));
}

#[test]
fn compares_dotted_versions() {
    assert_eq!(
        compare_dotted_versions("2026.05.24", "2025.12.31"),
        Some(std::cmp::Ordering::Greater)
    );
    assert_eq!(
        compare_dotted_versions("2026.05.24", "2026.05.24"),
        Some(std::cmp::Ordering::Equal)
    );
    assert_eq!(
        compare_dotted_versions("2025.01.01", "2026.05.24"),
        Some(std::cmp::Ordering::Less)
    );
}

#[test]
fn runtime_tool_replaces_a_same_size_corrupt_ytdlp_copy() {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("binaries")
        .join(format!("yt-dlp-{TARGET_TRIPLE}.exe"));
    let dest = std::env::temp_dir().join(format!(
        "mediadrop-corrupt-ytdlp-{}.exe",
        uuid::Uuid::new_v4()
    ));
    fs::copy(&source, &dest).expect("bundled yt-dlp should be copied for the test");
    let mut file = fs::OpenOptions::new().write(true).open(&dest).unwrap();
    std::io::Write::write_all(&mut file, b"not-a-valid-executable").unwrap();
    drop(file);
    assert_eq!(file_size(&source), file_size(&dest));

    let should_copy = should_copy_runtime_tool("yt-dlp", &source, &dest);
    let _ = fs::remove_file(&dest);
    assert!(should_copy, "a corrupt runtime copy must be replaced");
}

#[test]
fn report_sanitizer_removes_all_url_query_fragment_and_profile_values() {
    let text = sanitize_report_text(
        "https://example.test/video?id=abc&sig=secret&n=token&keep=value#frag https://rr2---sn-ab5szn7s.googlevideo.com/videoplayback?expire=123&sig=signed-googlevideo-token&lsig=another-token C:\\Users\\FakeProfile\\AppData\\Local\\Browser",
    );

    assert!(text.contains("https://example.test/video"));
    assert!(!text.contains("id=abc"));
    assert!(!text.contains("sig=secret"));
    assert!(!text.contains("keep=value"));
    assert!(!text.contains("#frag"));
    assert!(!text.contains("secret"));
    assert!(!text.contains("token"));
    assert!(!text.contains("googlevideo.com"));
    assert!(!text.contains("signed-googlevideo-token"));
    assert!(!text.contains("another-token"));
    assert!(!text.contains("FakeProfile"));
}

#[test]
fn supported_media_url_detection_uses_url_host() {
    assert!(is_youtube_url("https://youtu.be/abc123"));
    assert!(is_youtube_url("youtube.com/watch?v=abc123"));
    assert!(is_instagram_url("https://www.instagram.com/reel/abc123/"));
    assert!(is_twitter_url("https://x.com/user/status/123"));
    assert!(is_tiktok_url("https://vm.tiktok.com/abc123/"));
    assert!(is_tiktok_url(
        "https://www.tiktok.com/@creator/video/1234567890"
    ));
    assert!(!is_supported_media_url(
        "https://example.com/watch?next=youtube.com"
    ));
}

#[test]
fn tiktok_rehydration_retry_only_matches_the_transient_extractor_failure() {
    let url = "https://www.tiktok.com/@lolbert.3/video/7672947206864866590";
    let transient = "ERROR: [TikTok] 7672947206864866590: Unable to extract universal data for rehydration; please report";

    assert!(should_retry_tiktok_rehydration_error(url, transient, 1));
    assert!(should_retry_tiktok_rehydration_error(url, transient, 5));
    assert!(!should_retry_tiktok_rehydration_error(url, transient, 6));
    assert!(!should_retry_tiktok_rehydration_error(
        url,
        "ERROR: [TikTok] This video is private",
        1
    ));
    assert!(!should_retry_tiktok_rehydration_error(
        "https://www.youtube.com/watch?v=abc123",
        "Unable to extract universal data for rehydration",
        1
    ));
}

#[test]
fn gallery_json_normalizes_photo_carousel_and_story_items() {
    let stdout = r#"
{"url":"https://scontent.cdninstagram.com/photo1.jpg","extension":"jpg","width":1080,"height":1350,"caption":"Carousel title","id":"item-1"}
{"url":"https://scontent.cdninstagram.com/photo2.webp","extension":"webp","width":1080,"height":1080,"id":"item-1","filename":"second-photo"}
{"url":"https://scontent.cdninstagram.com/story.avif","extension":"avif","subcategory":"stories","id":"story-1"}
"#;

    let items = gallery_stdout_to_items(stdout, "instagram", "Instagram medyasi")
        .expect("gallery-dl JSON should normalize");

    assert_eq!(items.len(), 3);
    assert_eq!(items[0].item_type, "photo");
    assert_eq!(items[0].source_index, 0);
    assert_eq!(items[0].extension, "jpg");
    assert_eq!(items[0].width, Some(1080));
    assert_eq!(items[0].height, Some(1350));
    assert_eq!(items[1].source_index, 1);
    assert_eq!(items[1].extension, "webp");
    assert_eq!(items[0].id, "item-1");
    assert_eq!(items[1].id, "item-1-1");
    assert!(items[2].is_story);
    assert_eq!(items[2].extension, "avif");
    assert_eq!(media_content_kind(&items), "carousel");
}

#[test]
fn instagram_gallery_login_redirect_is_an_auth_failure() {
    let stdout = r#"[
      [-1, {
        "error": "AbortExtraction",
        "message": "HTTP redirect to login page (https://www.instagram.com/accounts/login/)"
      }]
    ]"#;

    let error = match gallery_stdout_to_inventory(stdout, "instagram", "Instagram gönderisi") {
        Err(error) => error,
        Ok(_) => panic!("login redirect must not become an empty media result"),
    };

    assert!(gallery_error_indicates_auth_failure(&error));
}

#[test]
fn gallery_json_filters_instagram_profile_images_from_media_items() {
    let stdout = r#"
{"url":"https://scontent.cdninstagram.com/v/t51.29350-15/post.jpg?stp=dst-jpg_e35","extension":"jpg","width":1080,"height":1350,"caption":"Post caption","username":"rockstargames","fullname":"Rockstar Games","profile_pic_url":"https://scontent.cdninstagram.com/v/t51.2885-19/avatar.jpg?stp=dst-jpg_s150x150","like_count":12}
{"url":"https://scontent.cdninstagram.com/v/t51.2885-19/avatar.jpg?stp=dst-jpg_s150x150","extension":"jpg","width":150,"height":150}
https://scontent.cdninstagram.com/v/t51.2885-19/avatar-large.jpg?stp=dst-jpg_s320x320
"#;

    let items = gallery_stdout_to_items(stdout, "instagram", "Instagram medyasi")
        .expect("gallery-dl JSON should keep only post media");

    assert_eq!(items.len(), 1);
    assert!(items[0].preview_url.contains("post.jpg"));
    assert_eq!(items[0].author_name.as_deref(), Some("Rockstar Games"));
    assert_eq!(items[0].author_handle.as_deref(), Some("rockstargames"));
    assert_eq!(items[0].text.as_deref(), Some("Post caption"));
    assert_eq!(items[0].like_count, Some(12));
    assert_eq!(
        items[0].avatar_url.as_deref(),
        Some("https://scontent.cdninstagram.com/v/t51.2885-19/avatar.jpg?stp=dst-jpg_s150x150")
    );
}

#[test]
fn gallery_json_preserves_twitter_text_post_without_media() {
    let stdout = r#"[
      [2, {
        "tweet_id": "2090883745628704991",
        "content": "The word attitude here refers to spacecraft position.",
        "date": "2026-08-21 19:28:26",
        "author": {
          "name": "NASA",
          "nick": "NASA",
          "verified": true
        },
        "reply_count": 3,
        "favorite_count": 42
      }]
    ]"#;

    let inventory = gallery_stdout_to_inventory(stdout, "twitter", "X medyasi")
        .expect("text-only Twitter metadata should remain usable");

    assert!(inventory.items.is_empty());
    let post = inventory
        .twitter_post
        .expect("text-only Twitter post metadata should be preserved");
    assert_eq!(post.id, "2090883745628704991");
    assert_eq!(post.text.as_deref(), Some("The word attitude here refers to spacecraft position."));
    assert_eq!(post.display_date.as_deref(), Some("2026-08-21 19:28:26"));
    assert_eq!(post.like_count, Some(42));
}

#[test]
fn instagram_official_flat_owner_identity_is_canonical_without_nested_owner() {
    let stdout = r#"
{"url":"https://scontent.cdninstagram.com/v/t51.29350-15/fixture-flat-owner.jpg","extension":"jpg","owner_id":"fixture-owner-001","username":"@Fixture_Owner","fullname":"Fixture Owner","profile_pic_url":"https://scontent.cdninstagram.com/v/t51.2885-19/fixture-flat-owner-avatar"}
"#;

    let items = gallery_stdout_to_items(stdout, "instagram", "Instagram medyasi")
        .expect("official flat owner fixture should parse");
    let identity = items[0]
        .canonical_instagram_identity
        .as_ref()
        .expect("official flat owner must become canonical");

    assert_eq!(identity.id.as_deref(), Some("fixture-owner-001"));
    assert_eq!(identity.handle.as_deref(), Some("fixture_owner"));
    assert_eq!(identity.name.as_deref(), Some("Fixture Owner"));
    assert!(identity
        .avatar_url
        .as_deref()
        .is_some_and(|url| url.ends_with("fixture-flat-owner-avatar")));
}

#[test]
fn instagram_flat_owner_rejects_empty_normalized_username_and_nested_id_mismatch() {
    let empty_handle = r#"
{"url":"https://scontent.cdninstagram.com/v/t51.29350-15/fixture-empty-handle.jpg","extension":"jpg","owner_id":"fixture-owner-001","username":" @ "}
"#;
    let mismatch = r#"
{"url":"https://scontent.cdninstagram.com/v/t51.29350-15/fixture-mismatch.jpg","extension":"jpg","owner_id":"fixture-owner-001","username":"fixture_owner","fullname":"Flat fallback must not win","profile_pic_url":"https://scontent.cdninstagram.com/v/t51.2885-19/fixture-flat-avatar","owner":{"pk":"fixture-owner-999","username":"fixture_other","profile_pic_url":"https://scontent.cdninstagram.com/v/t51.2885-19/fixture-other-avatar"}}
"#;

    let empty_items = gallery_stdout_to_items(empty_handle, "instagram", "Instagram medyasi")
        .expect("empty-handle fixture should remain parser compatible");
    assert!(empty_items[0].canonical_instagram_identity.is_none());

    let mismatch_items = gallery_stdout_to_items(mismatch, "instagram", "Instagram medyasi")
        .expect("mismatch fixture should remain parser compatible");
    assert!(mismatch_items[0].canonical_instagram_identity.is_none());
    assert_eq!(
        mismatch_items[0].author_handle.as_deref(),
        Some("fixture_owner")
    );
}

#[test]
fn instagram_root_owner_and_user_nodes_merge_only_when_ids_match() {
    let stdout = r#"
{"url":"https://scontent.cdninstagram.com/v/t50.2886-16/fixture-story.mp4","extension":"mp4","type":"story","subcategory":"stories","media_id":"1001","owner_id":"fixture-owner-001","username":"fixture_owner","owner":{"pk":"fixture-owner-001"},"user":{"pk":"fixture-owner-001","username":"fixture_owner","full_name":"Fixture Owner","profile_pic_url":"https://scontent.cdninstagram.com/v/t51.2885-19/fixture-owner-avatar"}}
"#;

    let items = gallery_stdout_to_items(stdout, "instagram", "Instagram hikayesi")
        .expect("matching root owner and user nodes should normalize");
    let identity = items[0]
        .canonical_instagram_identity
        .as_ref()
        .expect("matching root identity should exist");

    assert_eq!(identity.id.as_deref(), Some("fixture-owner-001"));
    assert_eq!(identity.handle.as_deref(), Some("fixture_owner"));
    assert_eq!(identity.name.as_deref(), Some("Fixture Owner"));
    assert!(identity
        .avatar_url
        .as_deref()
        .is_some_and(|url| url.ends_with("fixture-owner-avatar")));
}

#[test]
fn gallery_json_accepts_current_instagram_fbcdn_media_and_owner_avatar() {
    let stdout = r#"
[
  [2, {"post_id":"3936843886083820991","post_shortcode":"Daieqf2Mc2_","username":"burdurland"}],
  [3, "https://instagram.fyei5-1.fna.fbcdn.net/v/t51.82787-15/post-current.jpg?token=redacted", {
"display_url":"https://instagram.fyei5-1.fna.fbcdn.net/v/t51.82787-15/post-current.jpg?token=redacted",
"extension":"jpg",
"media_id":"3936843886083820991_36865112703",
"post_id":"3936843886083820991",
"post_shortcode":"Daieqf2Mc2_",
"type":"post",
"username":"burdurland",
"width":1179,
"height":1564,
"owner":{
  "username":"burdurland",
  "full_name":"Burdurland",
  "profile_pic_url":"https://instagram.fyei5-1.fna.fbcdn.net/v/t51.82787-15/owner-current.jpg?token=redacted"
}
  }]
]
"#;

    let items = gallery_stdout_to_items(stdout, "instagram", "Instagram medyasi")
        .expect("current gallery-dl Instagram schema should normalize");

    assert_eq!(items.len(), 1);
    assert!(items[0]
        .preview_url
        .contains("/v/t51.82787-15/post-current.jpg"));
    assert_eq!(items[0].author_handle.as_deref(), Some("burdurland"));
    assert_eq!(items[0].author_name.as_deref(), Some("Burdurland"));
    assert!(items[0]
        .avatar_url
        .as_deref()
        .is_some_and(|url| url.contains("/v/t51.82787-15/owner-current.jpg")));
}

#[test]
fn instagram_avatar_search_rejects_noncanonical_extended_account_metadata() {
    let value = json!({
        "url": "https://scontent.cdninstagram.com/v/t51.29350-15/post.jpg?stp=dst-jpg_e35",
        "extended": {
            "account": {
                "profile_pic_url_hd": "https://scontent.cdninstagram.com/v/t51.2885-19/avatar-hd.jpg?stp=dst-jpg_s320x320"
            }
        }
    });

    assert_eq!(find_instagram_avatar_url(&value), None);
}

#[test]
fn instagram_avatar_search_ignores_comment_authors_and_matches_post_owner() {
    let value = json!({
        "url": "https://scontent.cdninstagram.com/v/t51.29350-15/post.jpg?stp=dst-jpg_e35",
        "username": "post_owner",
        "comments": [
            {
                "user": {
                    "username": "comment_author",
                    "profile_pic_url": "https://scontent.cdninstagram.com/v/t51.2885-19/comment-avatar.jpg?stp=dst-jpg_s150x150"
                }
            }
        ],
        "owner": {
                "username": "post_owner",
                "profile_pic_url_hd": "https://scontent.cdninstagram.com/v/t51.2885-19/owner-avatar.jpg?stp=dst-jpg_s320x320"
        }
    });

    assert_eq!(
        find_instagram_avatar_url(&value).as_deref(),
        Some(
            "https://scontent.cdninstagram.com/v/t51.2885-19/owner-avatar.jpg?stp=dst-jpg_s320x320"
        )
    );
}

#[test]
fn instagram_avatar_search_does_not_fall_back_to_comment_author() {
    let value = json!({
        "url": "https://scontent.cdninstagram.com/v/t51.29350-15/post.jpg?stp=dst-jpg_e35",
        "username": "post_owner",
        "comments": [
            {
                "user": {
                    "username": "comment_author",
                    "profile_pic_url": "https://scontent.cdninstagram.com/v/t51.2885-19/comment-avatar.jpg?stp=dst-jpg_s150x150"
                }
            }
        ]
    });

    assert_eq!(find_instagram_avatar_url(&value), None);
}

#[test]
fn instagram_owner_id_mismatch_keeps_compatibility_metadata_out_of_final_author() {
    let stdout = r#"
{"url":"https://scontent.cdninstagram.com/v/t51.29350-15/post.jpg?stp=dst-jpg_e35","extension":"jpg","owner_id":"expected-owner","username":"compatibility_owner","fullname":"Compatibility Owner","profile_pic_url":"https://scontent.cdninstagram.com/v/t51.2885-19/compatibility-avatar.jpg","user":{"pk":"wrong-owner","username":"wrong_owner","profile_pic_url":"https://scontent.cdninstagram.com/v/t51.2885-19/wrong-avatar.jpg"}}
"#;
    let items = gallery_stdout_to_items(stdout, "instagram", "Instagram medyasi")
        .expect("compatibility metadata should still parse");
    assert_eq!(
        items[0].author_handle.as_deref(),
        Some("compatibility_owner")
    );
    assert!(items[0].canonical_instagram_identity.is_none());

    let finalized = finalize_media_analysis(
        MediaAnalysis {
            analysis_id: String::new(),
            expires_at_ms: 0,
            platform: "instagram".to_string(),
            content_kind: "photo".to_string(),
            title: "Fixture".to_string(),
            uploader: "Compatibility Owner".to_string(),
            author: AuthorIdentity::default(),
            items,
            initial_index: 0,
            requested_item_id: None,
            warnings: Vec::new(),
            instagram_diagnostics: None,
            twitter_quote: None,
            twitter_post: None,
            video_info: None,
        },
        "https://www.instagram.com/p/fixture/",
    );

    assert_eq!(finalized.author.id, None);
    assert!(finalized.author.name.is_empty());
    assert!(finalized.author.handle.is_empty());
    assert_eq!(finalized.author.avatar_data_url, None);
    assert!(finalized.items.iter().all(|item| {
        item.author_id.is_none()
            && item.author_name.is_none()
            && item.author_handle.is_none()
            && item.avatar_url.is_none()
            && item.avatar_data_url.is_none()
    }));
    let diagnostics = finalized
        .instagram_diagnostics
        .expect("Instagram diagnostics should be present");
    assert_eq!(diagnostics.author_source, "none");
    assert!(!diagnostics.identity_matched);
    assert!(!diagnostics.avatar_present);
}

#[test]
fn instagram_avatar_search_rejects_handles_empty_after_normalization() {
    let value = json!({
        "url": "https://scontent.cdninstagram.com/v/t51.29350-15/post.jpg?stp=dst-jpg_e35",
        "username": "@",
        "owner": {
            "username": "@",
            "profile_pic_url": "https://scontent.cdninstagram.com/v/t51.2885-19/avatar.jpg"
        }
    });

    assert_eq!(find_instagram_avatar_url(&value), None);
}

#[test]
fn instagram_avatar_resolver_policy_rejects_private_and_accepts_public_addresses() {
    let url = reqwest::Url::parse("https://scontent.cdninstagram.com/v/t51.2885-19/avatar.jpg")
        .expect("fixture URL should parse");

    assert!(validate_instagram_avatar_url_with_resolver(&url, |_, _| {
        Ok(vec!["127.0.0.1".parse().expect("fixture IP should parse")])
    })
    .is_err());
    assert!(validate_instagram_avatar_url_with_resolver(&url, |_, _| {
        Ok(vec!["93.184.216.34"
            .parse()
            .expect("fixture IP should parse")])
    })
    .is_ok());
}

#[test]
fn gallery_json_does_not_promote_nested_profile_picture_variants_to_post_media() {
    let stdout = r#"
{
  "url": "https://scontent.cdninstagram.com/v/t51.29350-15/post.jpg?stp=dst-jpg_e35",
  "extension": "jpg",
  "width": 1080,
  "height": 1350,
  "user": {
"username": "creator",
"profile_pic_url": "https://scontent.cdninstagram.com/v/t51.2885-19/avatar.jpg?stp=dst-jpg_s150x150",
"hd_profile_pic_versions": [
  {"url": "https://scontent.cdninstagram.com/v/t51.29350-15/user-image-one.jpg?stp=dst-jpg_e35", "width": 320, "height": 320},
  {"url": "https://scontent.cdninstagram.com/v/t51.29350-15/user-image-two.jpg?stp=dst-jpg_e35", "width": 640, "height": 640}
]
  }
}
"#;

    let items = gallery_stdout_to_items(stdout, "instagram", "Instagram medyasi")
        .expect("nested avatar variants must not become carousel items");

    assert_eq!(items.len(), 1);
    assert!(items[0].preview_url.contains("post.jpg"));
    assert_eq!(items[0].author_handle.as_deref(), Some("creator"));
    assert!(items[0]
        .avatar_url
        .as_deref()
        .is_some_and(|url| url.contains("t51.2885-19/avatar.jpg")));
}

#[test]
fn shared_post_metadata_is_propagated_across_carousel_items() {
    let stdout = r#"
{"url":"https://scontent.cdninstagram.com/v/t51.29350-15/one.jpg?stp=dst-jpg_e35","extension":"jpg","username":"creator","fullname":"Creator Name","profile_pic_url":"https://scontent.cdninstagram.com/v/t51.2885-19/avatar.jpg?stp=dst-jpg_s150x150","caption":"Shared caption"}
{"url":"https://scontent.cdninstagram.com/v/t51.29350-15/two.jpg?stp=dst-jpg_e35","extension":"jpg"}
"#;

    let mut items = gallery_stdout_to_items(stdout, "instagram", "Instagram medyasi")
        .expect("carousel JSON should normalize");
    propagate_media_item_metadata(&mut items);

    assert_eq!(items.len(), 2);
    assert_eq!(items[1].author_handle.as_deref(), Some("creator"));
    assert_eq!(items[1].author_name.as_deref(), Some("Creator Name"));
    assert_eq!(items[1].text.as_deref(), Some("Shared caption"));
    assert!(items[1]
        .avatar_url
        .as_deref()
        .is_some_and(|url| url.contains("t51.2885-19/avatar.jpg")));
}

#[test]
fn gallery_json_excludes_highlights_from_active_story_content() {
    let stdout = r#"
{"url":"https://scontent.cdninstagram.com/story1.jpg","extension":"jpg","subcategory":"stories","id":"story-1"}
{"url":"https://scontent.cdninstagram.com/story2.jpg","extension":"jpg","path":"highlights","id":"story-2"}
"#;

    let items = gallery_stdout_to_items(stdout, "instagram", "Instagram hikayesi")
        .expect("story JSON should normalize");

    assert_eq!(items.len(), 2);
    assert!(items[0].is_story);
    assert!(!items[1].is_story);
    assert_eq!(media_content_kind(&items), "carousel");
}

#[test]
fn instagram_highlight_gallery_records_are_not_active_stories() {
    assert!(instagram_highlight_url(
        "https://www.instagram.com/stories/highlights/1234567890/"
    ));
    let error = structured_backend_error(
        "instagram_highlight_unsupported",
        "Fixture highlight is unsupported",
    );
    assert_eq!(
        structured_backend_error_code(&error),
        Some("instagram_highlight_unsupported".to_string())
    );
    assert!(error.contains("instagram_highlight_unsupported"));
    let reported = format!("{}\n\nHata raporu oluşturuldu: C:\\Reports\\instagram.txt", error);
    assert_eq!(
        structured_backend_error_code(&reported),
        Some("instagram_highlight_unsupported".to_string())
    );

    let stdout = r#"
{"url":"https://scontent.cdninstagram.com/v/t51.29350-15/fixture-highlight.jpg","extension":"jpg","type":"highlight","id":"fixture-highlight"}
"#;
    let items = gallery_stdout_to_items(stdout, "instagram", "Instagram hikayesi")
        .expect("highlight fixture should parse as media only");
    assert!(!items[0].is_story);
}

#[test]
fn instagram_highlight_policy_short_circuits_before_any_collector_fallback() {
    let error = instagram_highlight_unsupported_error(
        "https://www.instagram.com/stories/highlights/1234567890/",
    )
    .expect("highlight policy must return before gallery, helper, or public fallback collection");
    let payload = serde_json::from_str::<serde_json::Value>(
        error
            .strip_prefix(STRUCTURED_ERROR_PREFIX)
            .expect("highlight policy must keep the structured error envelope"),
    )
    .expect("structured highlight error payload should be valid JSON");
    assert_eq!(payload["code"], "instagram_highlight_unsupported");
    assert_eq!(payload["action"], "instagram_highlight_unsupported");
    assert_eq!(payload["retryable"], false);
}

#[test]
fn gallery_resolve_json_keeps_mixed_story_photo_and_audio_video() {
    let stdout = include_str!("../tests/fixtures/instagram/mixed-unsorted-stories.jsonl");
    let mut items = gallery_stdout_to_items(stdout, "instagram", "Instagram hikayesi")
        .expect("resolved mixed Story JSON should normalize");
    items.sort_by_key(|item| item.taken_at_ms);

    assert_eq!(items.len(), 3);
    assert_eq!(items[0].id, "fixture-story-early");
    assert_eq!(items[0].item_type, "photo");
    assert_eq!(items[1].id, "fixture-story-middle");
    assert_eq!(items[1].duration_ms, Some(6500));
    assert!(!items[1].has_audio);
    assert_eq!(items[2].id, "fixture-story-late");
    assert_eq!(items[2].item_type, "video");
    assert!(items[2].has_audio);
    assert_eq!(items[2].duration_ms, Some(9000));
    assert_eq!(selected_media_items(&items, "all-stories").len(), 3);
}

#[test]
fn gallery_resolve_json_uses_message_url_not_story_display_cover() {
    let stdout = r#"
[
  [2, {"type":"story","subcategory":"stories"}],
  [3, "https://scontent.cdninstagram.com/v/t50.2886-16/story-video.mp4", {
    "display_url":"https://scontent.cdninstagram.com/v/t51.29350-15/story-video-cover.jpg",
    "extension":"mp4",
    "media_id":"1001",
    "type":"story",
    "subcategory":"stories",
    "username":"fixture_owner",
    "owner":{"pk":"fixture-owner-001","username":"fixture_owner"}
  }],
  [3, "https://scontent.cdninstagram.com/v/t51.29350-15/story-photo.jpg", {
    "display_url":"https://scontent.cdninstagram.com/v/t51.29350-15/story-photo.jpg",
    "extension":"jpg",
    "media_id":"1002",
    "type":"story",
    "subcategory":"stories",
    "username":"fixture_owner",
    "owner":{"pk":"fixture-owner-001","username":"fixture_owner"}
  }]
]
"#;

    let items = gallery_stdout_to_items(stdout, "instagram", "Instagram hikayesi")
        .expect("resolved Story messages should normalize");

    assert_eq!(items.len(), 2);
    assert_eq!(items[0].id, "1001");
    assert_eq!(items[0].item_type, "video");
    assert!(items[0].preview_url.ends_with("story-video.mp4"));
    assert_eq!(items[1].id, "1002");
    assert_eq!(items[1].item_type, "photo");
    assert!(items[1].preview_url.ends_with("story-photo.jpg"));
}

#[test]
fn instagram_reel_ytdl_message_prefers_video_url_over_display_cover() {
    let stdout = r#"
[
  [2, {"type":"post","subcategory":"reel"}],
  [3, "ytdl:https://www.instagram.com/p/DcQ6gfkIv2i/1.mp4", {
    "display_url":"https://scontent.cdninstagram.com/v/t51.82787-15/reel-cover.jpg",
    "extension":"mp4",
    "media_id":"3967928591326576034",
    "subcategory":"reel",
    "type":"post",
    "video_url":"https://scontent.cdninstagram.com/o1/v/t2/f2/m86/reel-video.mp4"
  }]
]
"#;

    let items = gallery_stdout_to_items(stdout, "instagram", "Instagram medyasi")
        .expect("resolved Reel message should normalize");

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].item_type, "video");
    assert_eq!(
        items[0].preview_url,
        "https://scontent.cdninstagram.com/o1/v/t2/f2/m86/reel-video.mp4"
    );
    assert_eq!(
        items[0].poster_ref.as_deref(),
        Some("https://scontent.cdninstagram.com/v/t51.82787-15/reel-cover.jpg")
    );
}

#[test]
fn instagram_share_request_never_uses_share_token_as_media_id() {
    assert_eq!(
        instagram_story_request("https://www.instagram.com/s/fixture-share-token/"),
        Some(InstagramStoryRequest::Share {
            token: "fixture-share-token".to_string(),
            query_media_id: None,
        })
    );
    assert_eq!(
        instagram_story_request(
            "https://www.instagram.com/s/fixture-share-token/?story_media_id=1002"
        ),
        Some(InstagramStoryRequest::Share {
            token: "fixture-share-token".to_string(),
            query_media_id: Some("1002".to_string()),
        })
    );
    assert_eq!(
        instagram_story_request("https://www.instagram.com/stories/fixture_owner/1002/"),
        Some(InstagramStoryRequest::Direct {
            username: "fixture_owner".to_string(),
            requested_media_id: Some("1002".to_string()),
        })
    );
}

#[test]
fn instagram_story_profile_url_without_item_id_enters_story_mode() {
    assert!(matches!(
        instagram_story_request("https://www.instagram.com/stories/fixture_owner/"),
        Some(InstagramStoryRequest::Direct { username, .. })
            if username == "fixture_owner"
    ));
}

#[test]
fn instagram_share_fixture_resolves_canonical_owner_and_numeric_media_id() {
    let stdout = include_str!("../tests/fixtures/instagram/share-resolved-target.jsonl");
    let items = gallery_stdout_to_items(stdout, "instagram", "Instagram hikayesi")
        .expect("share target fixture should parse");
    let target = resolve_instagram_share_story_target(&items, None)
        .expect("canonical share target should resolve");

    assert_eq!(target.username, "fixture_owner");
    assert_eq!(target.media_id, "1002");
    assert_eq!(
        instagram_story_profile_url(&target.username).as_deref(),
        Ok("https://www.instagram.com/stories/fixture_owner")
    );
    assert!(!items[0]
        .author_handle
        .as_deref()
        .is_some_and(|handle| handle.contains("wrong")));
}

#[test]
fn instagram_story_owner_rejects_compatibility_handle_without_canonical_marker() {
    let stdout = r#"
{"url":"https://scontent.cdninstagram.com/v/t51.29350-15/fixture-compat-story.jpg","extension":"jpg","type":"image","subcategory":"stories","id":"1002","username":"fixture_owner","taken_at":1730000200}
"#;
    let items = gallery_stdout_to_items(stdout, "instagram", "Instagram hikayesi")
        .expect("compatibility Story fixture should parse");
    assert!(items[0].canonical_instagram_identity.is_none());
    assert!(resolve_instagram_share_story_target(&items, None).is_err());
    assert!(canonical_owner_story_items(items, "fixture_owner").is_err());
}

#[test]
fn instagram_story_sort_uses_numeric_ids_after_taken_at() {
    let stdout = r#"
{"url":"https://scontent.cdninstagram.com/v/t51.29350-15/fixture-story-10.jpg","extension":"jpg","type":"image","subcategory":"stories","id":"10","username":"fixture_owner","taken_at":1730000200,"owner":{"pk":"fixture-owner-001","username":"fixture_owner"}}
{"url":"https://scontent.cdninstagram.com/v/t51.29350-15/fixture-story-2.jpg","extension":"jpg","type":"image","subcategory":"stories","id":"2","username":"fixture_owner","taken_at":1730000200,"owner":{"pk":"fixture-owner-001","username":"fixture_owner"}}
"#;
    let items = canonical_owner_story_items(
        gallery_stdout_to_items(stdout, "instagram", "Instagram hikayesi")
            .expect("numeric Story fixture should parse"),
        "fixture_owner",
    )
    .expect("canonical Story owners should match");
    let finalized = finalize_media_analysis(
        MediaAnalysis {
            analysis_id: String::new(),
            expires_at_ms: 0,
            platform: "instagram".to_string(),
            content_kind: "story".to_string(),
            title: "Fixture Story".to_string(),
            uploader: "Fixture Owner".to_string(),
            author: AuthorIdentity::default(),
            items,
            initial_index: 0,
            requested_item_id: None,
            warnings: Vec::new(),
            instagram_diagnostics: None,
            twitter_quote: None,
            twitter_post: None,
            video_info: None,
        },
        "https://www.instagram.com/stories/fixture_owner/2/",
    );
    assert_eq!(
        finalized
            .items
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        vec!["2", "10"]
    );
}

#[test]
fn instagram_story_fixtures_keep_success_and_non_auth_failures_distinct() {
    let photo = gallery_stdout_to_items(
        include_str!("../tests/fixtures/instagram/photo-story.json"),
        "instagram",
        "Instagram hikayesi",
    )
    .expect("photo Story fixture should parse");
    assert!(photo[0].is_story);
    assert_eq!(photo[0].item_type, "photo");

    let video = gallery_stdout_to_items(
        include_str!("../tests/fixtures/instagram/video-story-has-audio.json"),
        "instagram",
        "Instagram hikayesi",
    )
    .expect("video Story fixture should parse");
    assert!(video[0].is_story);
    assert_eq!(video[0].duration_ms, Some(12_000));
    assert!(video[0].has_audio);

    for stdout in [
        include_str!("../tests/fixtures/instagram/empty.json"),
        include_str!("../tests/fixtures/instagram/expired.json"),
        include_str!("../tests/fixtures/instagram/schema-error.json"),
    ] {
        let error = match gallery_stdout_to_items(stdout, "instagram", "Instagram hikayesi") {
            Ok(_) => panic!("empty, expired, and schema fixtures must have no media"),
            Err(error) => error,
        };
        assert_ne!(
            instagram_story_error_code(&error),
            Some("instagram_auth_required")
        );
    }
}

#[test]
fn instagram_share_active_fixture_filters_owner_sorts_and_selects_exact_target() {
    let stdout = include_str!("../tests/fixtures/instagram/share-owner-active-stories.jsonl");
    let items = gallery_stdout_to_items(stdout, "instagram", "Instagram hikayesi")
        .expect("active Story fixture should parse");
    let mut items = canonical_owner_story_items(items, "fixture_owner")
        .expect("active Story owner should match");
    for item in &mut items {
        item.avatar_url = None;
    }
    let analysis = MediaAnalysis {
        analysis_id: String::new(),
        expires_at_ms: 0,
        platform: "instagram".to_string(),
        content_kind: "story".to_string(),
        title: "Fixture Story".to_string(),
        uploader: "Fixture Owner".to_string(),
        author: AuthorIdentity::default(),
        items,
        initial_index: 0,
        requested_item_id: Some("1002".to_string()),
        warnings: Vec::new(),
        instagram_diagnostics: None,
        twitter_quote: None,
        twitter_post: None,
        video_info: None,
    };
    let finalized =
        finalize_media_analysis(analysis, "https://www.instagram.com/s/fixture-share-token/");

    assert_eq!(
        finalized
            .items
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        vec!["1001", "1002", "1003"]
    );
    assert_eq!(finalized.requested_item_id.as_deref(), Some("1002"));
    assert_eq!(finalized.initial_index, 1);
    assert_eq!(finalized.author.handle, "fixture_owner");
    assert!(finalized.items.iter().all(|item| {
        item.author_id.is_none()
            && item.author_name.is_none()
            && item.author_handle.is_none()
            && item.avatar_url.is_none()
            && item.avatar_data_url.is_none()
    }));
    assert!(!finalized
        .warnings
        .iter()
        .any(|warning| warning == "requestedStoryUnavailable"));
}

#[test]
fn instagram_share_owner_mismatch_is_rejected_and_missing_target_warns() {
    let mismatch = include_str!("../tests/fixtures/instagram/share-owner-mismatch.jsonl");
    let mismatch_items = gallery_stdout_to_items(mismatch, "instagram", "Instagram hikayesi")
        .expect("mismatch fixture should parse");
    let mismatch_error = canonical_owner_story_items(mismatch_items, "fixture_owner")
        .err()
        .expect("owner mismatch must fail");
    assert!(mismatch_error.contains("owner eslesmesi"));

    let stdout = include_str!("../tests/fixtures/instagram/share-owner-active-stories.jsonl");
    let mut items = canonical_owner_story_items(
        gallery_stdout_to_items(stdout, "instagram", "Instagram hikayesi")
            .expect("active Story fixture should parse"),
        "fixture_owner",
    )
    .expect("active Story owner should match");
    for item in &mut items {
        item.avatar_url = None;
    }
    let finalized = finalize_media_analysis(
        MediaAnalysis {
            analysis_id: String::new(),
            expires_at_ms: 0,
            platform: "instagram".to_string(),
            content_kind: "story".to_string(),
            title: "Fixture Story".to_string(),
            uploader: "Fixture Owner".to_string(),
            author: AuthorIdentity::default(),
            items,
            initial_index: 0,
            requested_item_id: Some("9999".to_string()),
            warnings: Vec::new(),
            instagram_diagnostics: None,
            twitter_quote: None,
            twitter_post: None,
            video_info: None,
        },
        "https://www.instagram.com/s/fixture-share-token/",
    );
    assert_eq!(finalized.initial_index, 0);
    assert_eq!(finalized.requested_item_id.as_deref(), Some("9999"));
    assert!(finalized
        .warnings
        .iter()
        .any(|warning| warning == "requestedStoryUnavailable"));
}

#[test]
fn canonical_instagram_author_fixture_never_selects_commenter() {
    let stdout = include_str!("../tests/fixtures/instagram/root-owner-commenter-trap.json");
    let items = gallery_stdout_to_items(stdout, "instagram", "Instagram medyasi")
        .expect("canonical owner fixture should parse");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].author_id.as_deref(), Some("fixture-owner-001"));
    assert_eq!(items[0].author_handle.as_deref(), Some("fixture_owner"));
    assert!(items[0]
        .avatar_url
        .as_deref()
        .is_some_and(|url| url.contains("fixture-root-owner-avatar")));
    assert!(!items[0]
        .avatar_url
        .as_deref()
        .is_some_and(|url| url.contains("commenter")));
}

#[test]
fn canonical_instagram_author_accepts_extensionless_avatar_fixture() {
    let stdout = include_str!("../tests/fixtures/instagram/extensionless-avatar.json");
    let items = gallery_stdout_to_items(stdout, "instagram", "Instagram medyasi")
        .expect("extensionless avatar fixture should parse");
    let avatar = items[0]
        .avatar_url
        .as_deref()
        .expect("canonical owner avatar should be present");
    assert!(avatar.ends_with("fixture-avatar-without-extension"));
    assert!(instagram_avatar_url_allowed(avatar));
}

#[test]
fn extensionless_instagram_avatar_response_requires_image_mime_and_matching_magic() {
    let extensionless = reqwest::Url::parse(
        "https://scontent.cdninstagram.com/v/t51.2885-19/fixture-avatar-without-extension",
    )
    .expect("fixture URL should parse");
    assert_eq!(avatar_mime_from_url(&extensionless), None);
    assert_eq!(avatar_mime_from_text("image/png"), Some("image/png"));
    assert!(avatar_bytes_match_mime(
        b"\x89PNG\r\n\x1a\nfixture",
        "image/png"
    ));
    assert_eq!(avatar_mime_from_text("text/html"), None);
    assert!(!avatar_bytes_match_mime(
        b"<html>fixture</html>",
        "image/png"
    ));
    assert!(!avatar_bytes_match_mime(b"not-an-image", "image/jpeg"));
}

#[test]
fn gallery_json_accepts_twitter_media_url_https() {
    let stdout = r#"
{"media_url_https":"https://pbs.twimg.com/media/ABC123.jpg","type":"photo","id_str":"2067219542841938261","original_info":{"width":2048,"height":1365},"content":"X photo post","author":{"nick":"Richard","name":"richarddcan","profile_image":"https://pbs.twimg.com/profile_images/avatar.jpg"},"favorite_count":42,"reply_count":3,"retweet_count":5}
"#;

    let items =
        gallery_stdout_to_items(stdout, "twitter", "X medyasi").expect("twitter photo JSON");

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].item_type, "photo");
    assert_eq!(items[0].extension, "jpg");
    assert_eq!(items[0].width, Some(2048));
    assert_eq!(items[0].height, Some(1365));
    assert_eq!(
        items[0].preview_url,
        "https://pbs.twimg.com/media/ABC123.jpg"
    );
    assert_eq!(items[0].author_name.as_deref(), Some("Richard"));
    assert_eq!(items[0].author_handle.as_deref(), Some("richarddcan"));
    assert_eq!(
        items[0].avatar_url.as_deref(),
        Some("https://pbs.twimg.com/profile_images/avatar.jpg")
    );
    assert_eq!(items[0].text.as_deref(), Some("X photo post"));
    assert_eq!(items[0].like_count, Some(42));
    assert_eq!(items[0].reply_count, Some(3));
    assert_eq!(items[0].retweet_count, Some(5));
}

#[test]
fn twitter_video_metadata_prefers_video_url_over_thumbnail() {
    let stdout = r#"
{"type":"video","video_url":"https://video.twimg.com/ext_tw_video/clip.mp4","thumbnail_url":"https://pbs.twimg.com/ext_tw_video_thumb/clip/pu/img/poster.jpg","extension":"mp4","id_str":"video-1"}
"#;

    let items =
        gallery_stdout_to_items(stdout, "twitter", "X videosu").expect("twitter video JSON");

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].item_type, "video");
    assert_eq!(
        items[0].preview_url,
        "https://video.twimg.com/ext_tw_video/clip.mp4"
    );
    assert_eq!(
        items[0].poster_ref.as_deref(),
        Some("https://pbs.twimg.com/ext_tw_video_thumb/clip/pu/img/poster.jpg")
    );
}

#[test]
fn twitter_preview_message_attaches_to_the_preceding_video() {
    let stdout = r#"[
  [3, "https://video.twimg.com/ext_tw_video/clip.mp4", {
    "type":"video",
    "extension":"mp4",
    "tweet_id":"123",
    "id":"video-1"
  }],
  [3, "https://pbs.twimg.com/ext_tw_video_thumb/clip/pu/img/poster.jpg", {
    "type":"preview",
    "extension":"jpg",
    "tweet_id":"123"
  }]
]"#;

    let items =
        gallery_stdout_to_items(stdout, "twitter", "X videosu").expect("twitter preview JSON");

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].item_type, "video");
    assert_eq!(
        items[0].poster_ref.as_deref(),
        Some("https://pbs.twimg.com/ext_tw_video_thumb/clip/pu/img/poster.jpg")
    );
}

#[test]
fn twitter_video_uses_post_text_instead_of_generated_filename() {
    let stdout = r#"[
  [3, "https://video.twimg.com/ext_tw_video/clip.mp4", {
    "type":"video",
    "extension":"mp4",
    "filename":"RkzNjf64SZ6X35d-",
    "content":"Gerçek gönderi metni",
    "tweet_id":"123",
    "author":{"nick":"Örnek Yazar","name":"ornek"}
  }]
]"#;

    let items =
        gallery_stdout_to_items(stdout, "twitter", "X medyası").expect("twitter video JSON");

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].title, "Gerçek gönderi metni");
}

#[test]
fn twitter_video_accepts_fractional_duration_seconds() {
    let stdout = r#"[
  [3, "https://video.twimg.com/ext_tw_video/clip.mp4", {
    "type":"video",
    "extension":"mp4",
    "duration":6.673,
    "tweet_id":"123"
  }]
]"#;

    let items =
        gallery_stdout_to_items(stdout, "twitter", "X medyası").expect("twitter video JSON");

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].duration_ms, Some(6_673));
}

#[test]
fn tiktok_video_metadata_keeps_cover_for_companion_preview() {
    let stdout = r#"
{"type":"video","video_url":"https://v16.tiktokcdn.com/video/clip.mp4","video":{"cover":"https://p16.tiktokcdn.com/obj/cover.jpg"},"extension":"mp4","id":"video-1"}
"#;

    let items =
        gallery_stdout_to_items(stdout, "tiktok", "TikTok videosu").expect("tiktok video JSON");

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].item_type, "video");
    assert_eq!(
        items[0].poster_ref.as_deref(),
        Some("https://p16.tiktokcdn.com/obj/cover.jpg")
    );
}

#[test]
fn tiktok_resolved_video_does_not_treat_bitrate_url_lists_as_photos() {
    let stdout = r#"[
      [2, {
        "video": {
          "PlayAddrStruct": {
            "UrlList": [
              "https://v19-webapp-prime.tiktok.com/video/original-a",
              "https://v19-webapp-prime.tiktok.com/video/original-b"
            ]
          },
          "bitrateInfo": [{
            "PlayAddr": {
              "UrlList": [
                "https://v19-webapp-prime.tiktok.com/video/variant-a",
                "https://v19-webapp-prime.tiktok.com/video/variant-b"
              ]
            }
          }],
          "claInfo": {
            "captionInfos": [{
              "url": "https://v16-webapp.tiktok.com/caption/generated"
            }]
          }
        }
      }],
      [3, "https://v16-webapp-prime.tiktok.com/video/canonical", {
        "type": "video",
        "extension": "mp4",
        "id": "video-1",
        "cover_url": "https://p16.tiktokcdn.com/obj/cover.jpg",
        "video": {
          "PlayAddrStruct": {
            "UrlList": [
              "https://v19-webapp-prime.tiktok.com/video/rejected",
              "https://v19-webapp-prime.tiktok.com/video/working"
            ]
          }
        }
      }]
    ]"#;

    let items = gallery_stdout_to_items(stdout, "tiktok", "TikTok videosu")
        .expect("resolved TikTok video JSON");

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id, "video-1");
    assert_eq!(items[0].item_type, "video");
    assert_eq!(
        items[0].preview_url,
        "https://v19-webapp-prime.tiktok.com/video/working"
    );
}

#[test]
fn twitter_quote_inventory_pairs_outer_and_direct_quote_without_flattening_authors() {
    let stdout = r#"[
  [2, {
    "tweet_id":"200",
    "quote_id":0,
    "content":"Dış tweet yorumu",
    "date":"2026-08-20 12:00:00",
    "author":{"nick":"Dış Yazar","name":"dis_yazar","profile_image":"https://pbs.twimg.com/profile_images/outer.jpg"},
    "favorite_count":20,
    "reply_count":2,
    "retweet_count":4
  }],
  [3, "https://pbs.twimg.com/media/OUTER?format=jpg&name=orig", {
    "tweet_id":"200",
    "quote_id":0,
    "media_id":"outer-media",
    "extension":"jpg",
    "type":"photo",
    "content":"Dış tweet yorumu",
    "author":{"nick":"Dış Yazar","name":"dis_yazar"}
  }],
  [2, {
    "tweet_id":"100",
    "quote_id":"200",
    "quote_by":"dis_yazar",
    "content":"Alıntılanan tweet metni",
    "date":"2026-08-19 18:30:00",
    "author":{"nick":"Alıntı Yazarı","name":"alinti_yazari","profile_image":"https://pbs.twimg.com/profile_images/quoted.jpg"},
    "favorite_count":120,
    "reply_count":12,
    "retweet_count":24
  }],
  [3, "https://video.twimg.com/ext_tw_video/QUOTE/vid/720x720/clip.mp4", {
    "tweet_id":"100",
    "quote_id":"200",
    "media_id":"quoted-media",
    "extension":"mp4",
    "type":"video",
    "content":"Alıntılanan tweet metni",
    "author":{"nick":"Alıntı Yazarı","name":"alinti_yazari"}
  }]
]"#;

    let inventory = gallery_stdout_to_inventory(stdout, "twitter", "X medyası")
        .expect("quote inventory should normalize");
    let quote = inventory
        .twitter_quote
        .expect("direct quote relationship should be retained");

    assert_eq!(inventory.items.len(), 2);
    assert_eq!(quote.outer.id, "200");
    assert_eq!(quote.outer.author_name, "Dış Yazar");
    assert_eq!(quote.outer.text.as_deref(), Some("Dış tweet yorumu"));
    assert_eq!(quote.quoted.id, "100");
    assert_eq!(quote.quoted.author_handle, "alinti_yazari");
    assert_eq!(quote.quoted.text.as_deref(), Some("Alıntılanan tweet metni"));
    assert_eq!(quote.quoted_media_indexes, vec![1]);
}

#[test]
fn twitter_post_registry_source_selects_the_requested_quoted_video() {
    let items = gallery_stdout_to_items(
        r#"[
          [3,"https://video.twimg.com/outer.mp4",{"media_id":"outer-video","type":"video","extension":"mp4"}],
          [3,"https://video.twimg.com/quoted.mp4",{"media_id":"quoted-video","type":"video","extension":"mp4"}]
        ]"#,
        "twitter",
        "X videosu",
    )
    .expect("Twitter video fixtures should parse");

    let selected = twitter_post_registry_source_item("twitter", &items, "quoted-video")
        .expect("quoted video should be selected by registry id");

    assert_eq!(selected.id, "quoted-video");
    assert_eq!(selected.preview_url, "https://video.twimg.com/quoted.mp4");
    assert!(twitter_post_registry_source_item("instagram", &items, "quoted-video").is_err());
    assert!(twitter_post_registry_source_item("twitter", &items, "missing").is_err());
}

#[test]
fn twitter_text_only_quote_ignores_author_website_urls() {
    let stdout = r#"[
  [2, {"tweet_id":"200","quote_id":0,"content":"Dış metin","author":{"nick":"Dış","name":"dis","url":"https://science.nasa.gov/mars/"}}],
  [2, {"tweet_id":"100","quote_id":"200","content":"Alıntı metni","author":{"nick":"Alıntı","name":"alinti","url":"http://global.jaxa.jp/"}}]
]"#;

    let inventory = gallery_stdout_to_inventory(stdout, "twitter", "X gönderisi")
        .expect("text-only quote should remain a valid inventory");

    assert!(inventory.items.is_empty());
    assert!(inventory.twitter_quote.is_some());
}

#[test]
fn ordinary_twitter_inventory_never_enters_quote_mode() {
    let stdout = r#"[
  [2, {"tweet_id":"200","quote_id":0,"content":"Normal tweet","author":{"nick":"Yazar","name":"yazar"}}],
  [3, "https://pbs.twimg.com/media/NORMAL?format=jpg&name=orig", {"tweet_id":"200","quote_id":0,"media_id":"normal-media","extension":"jpg","type":"photo"}]
]"#;

    let inventory = gallery_stdout_to_inventory(stdout, "twitter", "X medyası")
        .expect("ordinary tweet inventory should normalize");

    assert_eq!(inventory.items.len(), 1);
    assert!(inventory.twitter_quote.is_none());
}

#[test]
fn gallery_json_accepts_resolve_json_url_messages() {
    let stdout = r#"[
  [
2,
{
  "category": "twitter",
  "extension": "jpg",
  "filename": "ABCDE",
  "subcategory": "image"
}
  ],
  [
3,
"https://pbs.twimg.com/media/ABCDE?format=jpg&name=orig",
{
  "category": "twitter",
  "extension": "jpg",
  "filename": "ABCDE",
  "subcategory": "image"
}
  ]
]"#;

    let items =
        gallery_stdout_to_items(stdout, "twitter", "X medyasi").expect("resolve-json output");

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].item_type, "photo");
    assert_eq!(items[0].extension, "jpg");
    assert_eq!(
        items[0].preview_url,
        "https://pbs.twimg.com/media/ABCDE?format=jpg&name=orig"
    );
}

#[test]
fn gallery_stdout_accepts_raw_url_lines_without_fallback_duplicates() {
    let stdout = r#"
https://pbs.twimg.com/media/ABCDE?format=jpg&name=orig
| https://pbs.twimg.com/media/ABCDE?format=jpg&name=large
"#;

    let items = gallery_stdout_to_items(stdout, "twitter", "X medyasi").expect("raw URL output");

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].extension, "jpg");
    assert_eq!(
        items[0].preview_url,
        "https://pbs.twimg.com/media/ABCDE?format=jpg&name=orig"
    );
}

#[test]
fn instagram_public_post_url_strips_tracking_query() {
    assert_eq!(
        instagram_public_post_url(
            "https://www.instagram.com/p/DZpXKzCCnR/?utm_source=ig_web_copy_link&igsh=abc"
        )
        .as_deref(),
        Some("https://www.instagram.com/p/DZpXKzCCnR/")
    );
    assert!(instagram_public_post_url("https://www.instagram.com/stories/disney/123").is_none());
    assert_eq!(
        instagram_public_json_url("https://www.instagram.com/p/DZpXKzCCnR/").as_deref(),
        Some("https://www.instagram.com/p/DZpXKzCCnR/?__a=1&__d=dis")
    );
    assert_eq!(
        instagram_public_embed_urls("https://www.instagram.com/p/DZpXKzCCnR/")
            .into_iter()
            .map(|(_, url)| url)
            .collect::<Vec<_>>(),
        vec![
            "https://www.instagram.com/p/DZpXKzCCnR/embed/".to_string(),
            "https://www.instagram.com/p/DZpXKzCCnR/embed/captioned/".to_string(),
        ]
    );
    assert_eq!(
        instagram_public_oembed_url("https://www.instagram.com/p/DZpXKzCCnR/")
            .as_deref(),
        Some("https://www.instagram.com/oembed/?url=https%3A%2F%2Fwww.instagram.com%2Fp%2FDZpXKzCCnR%2F")
    );
}

#[test]
fn media_platform_from_url_uses_internal_platform_keys() {
    assert_eq!(
        media_platform_from_url("https://www.instagram.com/p/DZpXKzCCnR/"),
        "instagram"
    );
    assert_eq!(
        media_platform_from_url("https://x.com/user/status/123"),
        "twitter"
    );
    assert_eq!(
        media_platform_from_url("https://www.tiktok.com/@u/video/123"),
        "tiktok"
    );
}

#[test]
fn instagram_public_html_extracts_post_images_without_profile_avatar() {
    let html = r#"
<html><head>
<meta property="og:title" content="Disney Studios Turkiye on Instagram: Test post">
<meta property="og:image" content="https://scontent.cdninstagram.com/v/t51.29350-15/one.jpg?stp=dst-jpg_e35&amp;_nc_ht=scontent.cdninstagram.com">
<meta property="og:image:width" content="1080">
<meta property="og:image:height" content="1350">
</head><body>
<script>
{"carousel_media":[{"display_url":"https:\/\/scontent.cdninstagram.com\/v\/t51.29350-15\/two.jpg?stp=dst-jpg_e35\u0026x=1"},{"profile_pic_url":"https:\/\/scontent.cdninstagram.com\/avatar.jpg?stp=dst-jpg"}]}
</script>
</body></html>
"#;

    let items = instagram_public_items_from_html(html, "Instagram medyasi");

    assert_eq!(items.len(), 2);
    assert!(items[0].preview_url.contains("one.jpg"));
    assert!(items[0].preview_url.contains("&_nc_ht="));
    assert_eq!(items[0].extension, "jpg");
    assert_eq!(items[0].width, Some(1080));
    assert_eq!(items[0].height, Some(1350));
    assert_eq!(
        items[0].author_name.as_deref(),
        Some("Disney Studios Turkiye")
    );
    assert_eq!(items[0].text.as_deref(), Some("Test post"));
    assert_eq!(items[0].avatar_url, None);
    assert!(items[1].preview_url.contains("two.jpg"));
    assert!(!items.iter().any(|item| item.preview_url.contains("avatar")));
}

#[test]
fn instagram_public_html_extracts_embed_image_src() {
    let html = r#"
<div class="EmbeddedMediaImage">
  <img src="https://scontent.cdninstagram.com/v/t51.29350-15/embed.jpg?stp=dst-jpg_e35">
</div>
"#;

    let items = instagram_public_items_from_html(html, "Instagram medyasi");

    assert_eq!(items.len(), 1);
    assert!(items[0].preview_url.contains("embed.jpg"));
}

#[test]
fn instagram_public_html_loose_fallback_ignores_static_assets() {
    let html = r#"
<script>
["https://static.cdninstagram.com/rsrc.php/yr/r/app.webp",
"https://scontent.cdninstagram.com/v/t51.29350-15/post-media.jpg?stp=dst-jpg_e35"]
</script>
"#;

    let items = instagram_public_items_from_html(html, "Instagram medyasi");

    assert_eq!(items.len(), 1);
    assert!(items[0].preview_url.contains("post-media.jpg"));
    assert!(!items[0].preview_url.contains("rsrc.php"));
    assert!(!instagram_cdn_image_url_allowed(
        "https://static.cdninstagram.com/rsrc.php/yr/r/app.webp"
    ));
}

#[test]
fn gallery_json_accepts_instagram_oembed_thumbnail_url() {
    let stdout = r#"{"thumbnail_url":"https://scontent.cdninstagram.com/v/t51.29350-15/oembed.jpg?stp=dst-jpg_e35","thumbnail_width":640,"thumbnail_height":800,"title":"oEmbed title"}"#;

    let items = gallery_stdout_to_items(stdout, "instagram", "Instagram medyasi")
        .expect("oEmbed-like JSON should normalize");

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].item_type, "photo");
    assert_eq!(items[0].extension, "jpg");
    assert_eq!(items[0].width, Some(640));
    assert_eq!(items[0].height, Some(800));
    assert_eq!(items[0].title, "oEmbed title");
}

#[test]
fn media_preview_urls_reject_local_and_private_targets() {
    assert!(media_url_host_allowed(
        &reqwest::Url::parse("https://pbs.twimg.com/media/example?format=jpg").unwrap()
    ));

    for url in [
        "http://pbs.twimg.com/media/example.jpg",
        "https://localhost/media.jpg",
        "https://127.0.0.1/media.jpg",
        "https://10.0.0.5/media.jpg",
        "https://192.168.1.20/media.jpg",
        "https://[::1]/media.jpg",
    ] {
        assert!(
            !media_url_host_allowed(&reqwest::Url::parse(url).unwrap()),
            "{url} should be rejected"
        );
    }
}

#[test]
fn gallery_story_keeps_separate_audio_only_in_backend_model() {
    let stdout = r#"{
        "url":"https://scontent.cdninstagram.com/v/story-video.mp4",
        "audio_url":"https://scontent.cdninstagram.com/v/story-audio.m4a",
        "extension":"mp4",
        "type":"video",
        "subcategory":"stories",
        "media_id":"987654321",
        "username":"fixture_owner",
        "has_audio":true
    }"#;
    let items = gallery_stdout_to_items(stdout, "instagram", "Story")
        .expect("story video with separate audio should normalize");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].item_type, "video");
    assert!(items[0].has_audio);
    assert_eq!(
        items[0].audio_url.as_deref(),
        Some("https://scontent.cdninstagram.com/v/story-audio.m4a")
    );

    let public_model = serde_json::to_value(&items[0]).expect("media item should serialize");
    assert!(public_model.get("previewUrl").is_none());
    assert!(public_model.get("audioUrl").is_none());
}

#[test]
fn media_extensions_and_batch_names_are_safe_for_windows() {
    assert!(supported_image_extension("jpg"));
    assert!(supported_image_extension("jpeg"));
    assert!(supported_image_extension("png"));
    assert!(supported_image_extension("webp"));
    assert!(supported_image_extension("gif"));
    assert!(supported_image_extension("avif"));
    assert_eq!(sanitize_media_extension("exe"), "jpg");

    let name = media_batch_dir_name("instagram", "CON <bad>| gallery???");

    assert!(name.starts_with("Instagram Fotograflari - "));
    assert!(!name.contains('<'));
    assert!(!name.contains('|'));
    assert!(!name.contains('?'));
}

#[test]
fn gallery_auth_attempts_never_scan_all_browsers_implicitly() {
    let labels: Vec<_> = gallery_auth_attempts(Some("browserAuto"))
        .into_iter()
        .map(|attempt| attempt.label())
        .collect();
    assert_eq!(labels, vec!["public"]);

    let public_labels: Vec<_> = gallery_auth_attempts(Some("public"))
        .into_iter()
        .map(|attempt| attempt.label())
        .collect();

    assert_eq!(public_labels, vec!["public"]);

    let default_labels: Vec<_> = gallery_auth_attempts(None)
        .into_iter()
        .map(|attempt| attempt.label())
        .collect();
    let unknown_labels: Vec<_> = gallery_auth_attempts(Some("unknown-mode"))
        .into_iter()
        .map(|attempt| attempt.label())
        .collect();
    assert_eq!(default_labels, vec!["public"]);
    assert_eq!(unknown_labels, vec!["public"]);
}

#[test]
fn gallery_auth_attempts_can_target_selected_browser_only() {
    let opera_labels: Vec<_> = gallery_auth_attempts(Some("browser:opera"))
        .into_iter()
        .map(|attempt| attempt.label())
        .collect();
    let chrome_labels: Vec<_> = gallery_auth_attempts(Some("browser:chrome"))
        .into_iter()
        .map(|attempt| attempt.label())
        .collect();
    let firefox_labels: Vec<_> = gallery_auth_attempts(Some("browser:firefox"))
        .into_iter()
        .map(|attempt| attempt.label())
        .collect();

    assert_eq!(opera_labels, vec!["opera instagram cookies"]);
    assert_eq!(chrome_labels, vec!["chrome instagram cookies"]);
    assert_eq!(firefox_labels, vec!["firefox instagram cookies"]);
}

#[test]
fn gallery_auth_attempts_support_saved_and_save_modes() {
    let twitter_labels: Vec<_> = gallery_auth_attempts(Some("registered:twitter"))
        .into_iter()
        .map(|attempt| attempt.label())
        .collect();
    assert_eq!(twitter_labels, vec!["registered X/Twitter cookies"]);

    let saved_labels: Vec<_> = gallery_auth_attempts(Some("saved:instagram"))
        .into_iter()
        .map(|attempt| attempt.label())
        .collect();
    assert_eq!(saved_labels, vec!["saved instagram cookies"]);

    let prepared_labels: Vec<_> = gallery_auth_attempts(Some("prepared:instagram:abc123"))
        .into_iter()
        .map(|attempt| attempt.label())
        .collect();
    assert_eq!(prepared_labels, vec!["prepared instagram cookies"]);

    let attempts = gallery_auth_attempts(Some("browser:opera_gx:save"));
    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0].label(), "opera gx instagram cookies");
    assert!(matches!(
        attempts[0],
        GalleryAuthAttempt::BrowserCookies {
            browser_id: "opera_gx",
            save: true,
            ..
        }
    ));
}

#[test]
fn instagram_auth_mode_only_treats_explicit_credentials_as_authenticated() {
    assert!(instagram_auth_mode_uses_credentials(Some(
        "saved:instagram"
    )));
    assert!(instagram_auth_mode_uses_credentials(Some(
        "prepared:instagram:token"
    )));
    assert!(instagram_auth_mode_uses_credentials(Some("browser:chrome")));
    assert!(instagram_auth_mode_uses_credentials(Some(
        "browser:opera_gx:save"
    )));

    assert!(!instagram_auth_mode_uses_credentials(None));
    assert!(!instagram_auth_mode_uses_credentials(Some("browserAuto")));
    assert!(!instagram_auth_mode_uses_credentials(Some("public")));
    assert!(!instagram_auth_mode_uses_credentials(Some(
        "prepared:instagram:"
    )));
    assert!(!instagram_auth_mode_uses_credentials(Some(
        "browser:unsupported"
    )));
}

#[test]
fn instaloader_policy_only_allows_post_extractor_schema_or_empty_media_failures() {
    let post_url = "https://www.instagram.com/p/fixture-post/";
    assert!(instagram_helper_fallback_allowed(
        post_url,
        "gallery-dl JSON sonucu okunamadi: schema mismatch"
    ));
    assert!(instagram_helper_fallback_allowed(
        post_url,
        "public: medyada indirilebilir gorsel bulunamadi"
    ));
    assert!(instagram_helper_fallback_allowed(
        post_url,
        &structured_backend_error("instagram_schema_error", "schema mismatch")
    ));
    assert!(instagram_helper_fallback_allowed(
        post_url,
        &structured_backend_error("instagram_media_empty", "media empty")
    ));

    assert!(!instagram_helper_fallback_allowed(
        post_url,
        "network connection timed out"
    ));
}

#[test]
fn instaloader_policy_blocks_auth_rate_not_found_access_and_expired_codes() {
    let post_url = "https://www.instagram.com/p/fixture-post/";
    for code in [
        "instagram_auth_required",
        "instagram_rate_limited",
        "instagram_story_not_found",
        "instagram_story_access_denied",
        "instagram_story_expired",
    ] {
        let error = structured_backend_error(code, "fixture failure");
        assert!(
            !instagram_helper_fallback_allowed(post_url, &error),
            "helper must stay disabled for {code}"
        );
    }

    for error in [
        "AuthenticationError: login required",
        "HTTP 429 Too Many Requests",
        "HTTP 404 not found",
        "Story expired",
        "private profile access denied",
    ] {
        assert!(
            !instagram_helper_fallback_allowed(post_url, error),
            "helper must stay disabled for {error}"
        );
    }
}

#[test]
fn story_never_uses_instaloader_even_for_schema_or_empty_media_failures() {
    let story_url = "https://www.instagram.com/stories/fixture_owner/123456/";
    assert!(!instagram_helper_fallback_allowed(
        story_url,
        "gallery-dl JSON sonucu okunamadi: schema mismatch"
    ));
    assert!(!instagram_helper_fallback_allowed(
        story_url,
        &structured_backend_error("instagram_media_empty", "media empty")
    ));
}

#[test]
fn authenticated_public_fallback_warning_is_explicit_and_deduplicated() {
    let mut analysis = MediaAnalysis {
        analysis_id: String::new(),
        expires_at_ms: 0,
        platform: "instagram".to_string(),
        content_kind: "photo".to_string(),
        title: "Fixture".to_string(),
        uploader: "Fixture".to_string(),
        author: AuthorIdentity::default(),
        items: Vec::new(),
        initial_index: 0,
        requested_item_id: None,
        warnings: Vec::new(),
        instagram_diagnostics: None,
        twitter_quote: None,
        twitter_post: None,
        video_info: None,
    };

    add_analysis_warning(
        &mut analysis,
        INSTAGRAM_AUTHENTICATED_PUBLIC_FALLBACK_WARNING,
    );
    add_analysis_warning(
        &mut analysis,
        INSTAGRAM_AUTHENTICATED_PUBLIC_FALLBACK_WARNING,
    );

    assert_eq!(
        analysis.warnings,
        vec![INSTAGRAM_AUTHENTICATED_PUBLIC_FALLBACK_WARNING]
    );
}

#[test]
fn gallery_auth_detection_ignores_metadata_words_and_requires_explicit_failure() {
    let metadata =
        r#"{"owner":{"is_private":false,"user_activation_info":{},"login_experiment":true}}"#;
    assert!(!gallery_error_indicates_auth_failure(metadata));
    assert!(gallery_error_indicates_auth_failure(
        "AuthenticationError: login required; provide a valid sessionid"
    ));
    assert!(gallery_error_indicates_auth_failure(
        "Kayitli Instagram cookie verisi gecersiz veya bos."
    ));
    assert!(!gallery_error_indicates_auth_failure(
        "gallery-dl JSON ciktisi indirilebilir medya URL'si icermedi"
    ));
}

#[test]
fn cookie_restart_classifier_only_accepts_real_lock_signals() {
    assert!(instagram_cookie_error_is_browser_lock(
        "database is locked (SQLITE_BUSY)"
    ));
    assert!(instagram_cookie_error_is_browser_lock(
        "The process cannot access the file because it is being used by another process (os error 32)"
    ));
    assert!(!instagram_cookie_error_is_browser_lock(
        "Secili tarayicida Instagram oturumu bulunamadi"
    ));
    assert!(!instagram_cookie_error_is_browser_lock(
        "Chromium cookie deÅŸifre edilemedi"
    ));
    assert!(!instagram_cookie_error_is_browser_lock(
        "gallery-dl JSON sonucu okunamadi"
    ));
}

#[test]
fn story_error_classifier_keeps_auth_parser_and_access_failures_distinct() {
    assert_eq!(
        instagram_story_error_code("HTTP 429 Too Many Requests"),
        Some("instagram_rate_limited")
    );
    assert_eq!(
        instagram_story_error_code("403 Forbidden: private profile"),
        Some("instagram_story_access_denied")
    );
    assert_eq!(
        instagram_story_error_code("gallery-dl JSON sonucu okunamadi"),
        Some("instagram_schema_error")
    );
    assert_eq!(
        instagram_story_error_code("No stories found; expired"),
        Some("instagram_story_not_found")
    );
    assert_eq!(
        instagram_story_error_code("AuthenticationError: login required"),
        Some("instagram_auth_required")
    );
}

#[test]
fn instagram_login_evidence_wins_over_a_403_but_private_403_stays_access_denied() {
    assert_eq!(
        instagram_story_error_code("HTTP 403 Forbidden after redirect to login page"),
        Some("instagram_auth_required")
    );
    assert_eq!(
        instagram_story_error_code("HTTP 403 Forbidden: private profile"),
        Some("instagram_story_access_denied")
    );
}

#[test]
fn instagram_cookie_boundary_errors_remain_typed() {
    assert_eq!(
        ApiError::from(instagram_cookie_boundary_error(
            "Kayitli Instagram oturumunun suresi dolmus."
        ))
        .code,
        "instagram_auth_expired"
    );
    assert_eq!(
        ApiError::from(instagram_cookie_boundary_error(
            "Chromium cookie deÅŸifre edilemedi."
        ))
        .code,
        "instagram_cookie_invalid"
    );
}

#[test]
fn instagram_saved_cookie_read_and_dpapi_failures_stay_typed_and_redacted() {
    for message in [
        "Kayitli cookie verisi bos.",
        "Kayitli Instagram cookie verisi acilamadi.",
        "Kayitli Instagram cookie verisi okunamadi: C:\\Users\\FakeProfile\\Cookies: access denied",
    ] {
        let error = ApiError::from(sanitize_report_text(&instagram_cookie_boundary_error(message)));
        assert_eq!(error.code, "instagram_cookie_invalid");
        assert_eq!(error.action.as_deref(), Some("request_cookie_permission"));
        assert!(!error.message.contains("FakeProfile"));
    }

    let typed = structured_backend_error("instagram_browser_locked", "Cookie database locked");
    assert_eq!(instagram_cookie_boundary_error(&typed), typed);
}

#[test]
fn instagram_batch_auth_failures_are_not_partial_results() {
    assert!(media_batch_failure_requires_auth_recovery(
        &structured_backend_error("instagram_auth_required", "Instagram login required")
    ));
    assert!(media_batch_failure_requires_auth_recovery(
        &structured_backend_error("instagram_auth_expired", "Saved session expired")
    ));
    assert!(!media_batch_failure_requires_auth_recovery(
        &structured_backend_error("instagram_story_access_denied", "Private profile")
    ));
}

#[test]
fn prepared_instagram_cookie_token_materializes_temp_jar() {
    let jar = BrowserCookieJar {
        browser_id: "opera_gx".to_string(),
        browser_label: "Opera GX".to_string(),
        profile_label: "Default/Network/Cookies".to_string(),
        cookies: vec![
            NetscapeCookie {
                domain: ".instagram.com".to_string(),
                include_subdomains: true,
                path: "/".to_string(),
                secure: true,
                expires: 1_900_000_000,
                name: "sessionid".to_string(),
                value: "prepared-session".to_string(),
            },
            NetscapeCookie {
                domain: ".instagram.com".to_string(),
                include_subdomains: true,
                path: "/".to_string(),
                secure: true,
                expires: 1_900_000_000,
                name: "ds_user_id".to_string(),
                value: "123456".to_string(),
            },
        ],
        score: 200,
        failed_decrypts: 0,
    };

    let token = store_prepared_instagram_cookie_jar(&jar).expect("token should be stored");
    let path = materialize_prepared_instagram_cookie_file(&token)
        .expect("prepared token should materialize");
    let text = fs::read_to_string(&path).expect("temp jar should be readable");
    let _ = fs::remove_file(&path);

    assert!(text.contains(".instagram.com\tTRUE\t/\tTRUE\t1900000000\tsessionid\tprepared-session"));
}

#[test]
fn saved_cookie_validation_requires_live_instagram_session_and_user_id() {
    let future = unix_time_seconds().saturating_add(3600);
    let past = unix_time_seconds().saturating_sub(3600);
    let ready = format!(
        ".instagram.com\tTRUE\t/\tTRUE\t{}\tsessionid\tfixture-session\n.instagram.com\tTRUE\t/\tTRUE\t{}\tds_user_id\t123456",
        future, future
    );
    let expired = format!(
        ".instagram.com\tTRUE\t/\tTRUE\t{}\tsessionid\tfixture-session\n.instagram.com\tTRUE\t/\tTRUE\t{}\tds_user_id\t123456",
        past, past
    );
    assert_eq!(
        validate_netscape_instagram_session(&ready),
        SavedInstagramCookieValidation::Ready
    );
    assert_eq!(
        validate_netscape_instagram_session(&expired),
        SavedInstagramCookieValidation::Expired
    );
    assert_eq!(
        validate_netscape_instagram_session(".instagram.com\tTRUE\t/\tTRUE\t0\tcsrftoken\tfixture"),
        SavedInstagramCookieValidation::Invalid
    );
    assert_eq!(
        validate_netscape_instagram_session(&format!(
            ".instagram.com\tTRUE\t/\tTRUE\t{}\tsessionid\tfixture-session",
            future
        )),
        SavedInstagramCookieValidation::Invalid
    );
}

#[test]
fn instagram_cookie_status_contract_uses_stable_machine_values() {
    let cases = [
        (InstagramCookieStatus::Missing, "missing"),
        (InstagramCookieStatus::Ready, "ready"),
        (InstagramCookieStatus::Expired, "expired"),
        (InstagramCookieStatus::Invalid, "invalid"),
        (InstagramCookieStatus::BrowserLocked, "browser_locked"),
    ];

    for (status, expected) in cases {
        assert_eq!(
            serde_json::to_value(status).expect("status should serialize"),
            serde_json::Value::String(expected.to_string())
        );
    }
}

#[test]
fn media_download_registry_identity_rejects_legacy_fallback_inputs() {
    assert_eq!(
        require_media_registry_identity(" analysis-1 ", " story-7 ")
            .expect("registry identity should be accepted"),
        ("analysis-1".to_string(), "story-7".to_string())
    );
    assert!(require_media_registry_identity("", "story-7").is_err());
    assert!(require_media_registry_identity("analysis-1", "").is_err());
}

#[test]
fn registry_refresh_policy_allows_only_one_explicit_signed_url_recovery() {
    for error in [
        "Gorsel indirme HTTP 401 Unauthorized dondu.",
        "Gorsel indirme HTTP 403 Forbidden dondu.",
        "Gorsel indirme HTTP 410 Gone dondu.",
    ] {
        assert!(media_error_allows_registry_refresh(error, 0));
        assert!(!media_error_allows_registry_refresh(error, 1));
    }

    assert!(!media_error_allows_registry_refresh(
        "Gorsel indirme baglanti zaman asimina ugradi.",
        0
    ));
    assert!(!media_error_allows_registry_refresh(
        "Story videosunda beklenen ses akisi bulunamadi.",
        0
    ));
}

#[test]
fn registry_refresh_claim_is_atomic_per_analysis_entry() {
    let mut refresh_attempted = false;
    assert!(claim_registry_refresh(&mut refresh_attempted));
    assert!(refresh_attempted);
    assert!(!claim_registry_refresh(&mut refresh_attempted));
}

#[test]
fn api_error_contract_serializes_fields_and_decodes_legacy_payloads() {
    let error = ApiError::new("download_busy", "Baska bir indirme suruyor.")
        .with_retryable(true)
        .with_action("wait_for_active_download");
    assert_eq!(
        serde_json::to_value(&error).expect("ApiError should serialize"),
        json!({
            "code": "download_busy",
            "message": "Baska bir indirme suruyor.",
            "retryable": true,
            "action": "wait_for_active_download",
            "reportId": null
        })
    );

    let legacy =
        structured_backend_error("instagram_auth_required", "Instagram oturumu gerekiyor.");
    let decoded = ApiError::from_legacy(legacy);
    assert_eq!(decoded.code, "instagram_auth_required");
    assert!(decoded.retryable);
    assert_eq!(decoded.action.as_deref(), Some("request_cookie_permission"));
}

#[test]
fn opera_gx_process_matching_does_not_target_regular_opera() {
    let gx_path = Path::new(r"C:\Users\User\AppData\Local\Programs\Opera GX\opera.exe");
    let opera_path = Path::new(r"C:\Users\User\AppData\Local\Programs\Opera\opera.exe");

    assert!(cookie_browser_process_path_matches(
        "opera_gx",
        Some(gx_path)
    ));
    assert!(!cookie_browser_process_path_matches(
        "opera_gx",
        Some(opera_path)
    ));
    assert!(cookie_browser_process_path_matches(
        "opera",
        Some(opera_path)
    ));
    assert!(!cookie_browser_process_path_matches("opera", Some(gx_path)));
}

#[test]
fn cookie_browser_listing_has_supported_ids() {
    let browsers = list_cookie_browsers();
    let ids: Vec<_> = browsers.iter().map(|browser| browser.id.as_str()).collect();

    assert_eq!(ids, vec!["opera_gx", "opera", "chrome", "edge", "firefox"]);
    assert_eq!(
        browsers
            .iter()
            .filter(|browser| browser.recommended)
            .count(),
        1
    );
    assert!(cookie_browser_id_from_progid("ChromeHTML").is_some());
    assert_eq!(cookie_browser_id_from_progid("MSEdgeHTM"), Some("edge"));
}

#[test]
fn youtube_age_gate_is_distinguished_from_private_video_errors() {
    assert!(youtube_browser_auth_required(
        "Sign in to confirm your age. This video may be inappropriate for some users."
    ));
    assert!(youtube_browser_auth_required(
        "This video is age-restricted and only available on YouTube"
    ));
    assert!(!youtube_browser_auth_required(
        "Private video. Sign in if you've been granted access"
    ));
}

#[test]
fn youtube_browser_cookie_read_failures_allow_another_browser_retry() {
    assert!(youtube_browser_cookie_read_failed(
        "ERROR: Could not copy Chrome cookie database. See yt-dlp FAQ for details"
    ));
    assert!(youtube_browser_cookie_read_failed(
        "ERROR: Failed to decrypt with DPAPI"
    ));
    assert!(!youtube_browser_cookie_read_failed(
        "Private video. Sign in if you've been granted access"
    ));
}

#[test]
fn youtube_cookie_database_lock_retries_the_same_running_browser() {
    let locked = youtube_cookie_analysis_error(
        "ERROR: Could not copy Chrome cookie database",
        Some("opera_gx"),
        true,
    )
    .expect("cookie lock should be classified");
    let parsed = ApiError::from_legacy(locked);

    assert_eq!(parsed.code, "browser_restart_required");
    assert_eq!(parsed.action.as_deref(), Some("request_browser_restart"));

    let unreadable = youtube_cookie_analysis_error(
        "ERROR: Failed to decrypt with DPAPI",
        Some("opera_gx"),
        true,
    )
    .expect("decrypt failure should be classified");
    assert_eq!(ApiError::from_legacy(unreadable).code, "youtube_auth_failed");
}

#[test]
fn prepared_youtube_cookies_are_materialized_only_for_the_command_lifetime() {
    let url = format!("https://www.youtube.com/watch?v={}", unique_stamp());
    let source = concat!(
        "# Netscape HTTP Cookie File\n",
        ".youtube.com\tTRUE\t/\tTRUE\t0\tSAPISID\tfixture-session\n",
        ".example.com\tTRUE\t/\tTRUE\t0\tunrelated\tsecret\n"
    );
    register_ytdlp_cookie_jar(&url, "chrome", source).unwrap();

    let mut command = Command::new("yt-dlp");
    let artifact = add_registered_ytdlp_cookies(&mut command, &url)
        .unwrap()
        .expect("registered cookie jar should materialize");
    let path = artifact.path().to_path_buf();
    let text = fs::read_to_string(&path).unwrap();
    let args = command
        .get_args()
        .map(|arg| arg.to_string_lossy().to_string())
        .collect::<Vec<_>>();

    assert_eq!(args.first().map(String::as_str), Some("--cookies"));
    assert_eq!(args.get(1).map(String::as_str), Some(path.to_string_lossy().as_ref()));
    assert!(text.contains(".youtube.com"));
    assert!(!text.contains(".example.com"));
    drop(artifact);
    assert!(!path.exists());
}

#[test]
fn prepared_twitter_cookies_are_scoped_and_materialized_only_for_the_command_lifetime() {
    let url = format!("https://x.com/fixture/status/{}", unique_stamp());
    let source = concat!(
        "# Netscape HTTP Cookie File\n",
        ".x.com\tTRUE\t/\tTRUE\t0\tauth_token\tfixture-session\n",
        ".x.com\tTRUE\t/\tTRUE\t0\tct0\tfixture-csrf\n",
        ".twitter.com\tTRUE\t/\tTRUE\t0\ttwid\tfixture-user\n",
        ".example.com\tTRUE\t/\tTRUE\t0\tunrelated\tsecret\n"
    );
    register_ytdlp_cookie_jar(&url, "opera", source).unwrap();

    let mut command = Command::new("gallery-dl");
    let artifact = add_registered_ytdlp_cookies(&mut command, &url)
        .unwrap()
        .expect("registered X cookie jar should materialize");
    let path = artifact.path().to_path_buf();
    let text = fs::read_to_string(&path).unwrap();
    let args = command
        .get_args()
        .map(|arg| arg.to_string_lossy().to_string())
        .collect::<Vec<_>>();

    assert_eq!(args.first().map(String::as_str), Some("--cookies"));
    assert_eq!(args.get(1).map(String::as_str), Some(path.to_string_lossy().as_ref()));
    assert!(text.contains(".x.com"));
    assert!(text.contains(".twitter.com"));
    assert!(!text.contains(".example.com"));
    drop(artifact);
    assert!(!path.exists());
    prepared_ytdlp_cookies().lock().unwrap().remove(&url);
}

#[test]
fn twitter_cookie_registration_requires_a_live_auth_token() {
    let url = format!("https://x.com/fixture/status/{}", unique_stamp());
    let source = concat!(
        "# Netscape HTTP Cookie File\n",
        ".x.com\tTRUE\t/\tTRUE\t0\tct0\tfixture-csrf\n"
    );

    assert!(register_ytdlp_cookie_jar(&url, "opera", source).is_err());
}

#[test]
fn twitter_placeholder_analysis_requires_auth_but_real_posts_do_not() {
    let placeholder = json!({
        "id": "2090384899064700955",
        "extractor": "twitter",
        "title": "twitter video #2090384899064700955",
        "description": "",
        "formats": []
    });
    let text_post = json!({
        "id": "42",
        "extractor": "twitter",
        "title": "Gerçek gönderi metni",
        "description": "Gerçek gönderi metni",
        "uploader": "MediaDrop",
        "formats": []
    });
    let video_post = json!({
        "id": "43",
        "extractor": "twitter",
        "title": "Video gönderisi",
        "formats": [{
            "format_id": "http-720",
            "url": "https://video.twimg.com/ext_tw_video/fixture.mp4",
            "ext": "mp4",
            "protocol": "https",
            "vcodec": "avc1"
        }]
    });

    assert!(twitter_ytdlp_analysis_is_placeholder(&placeholder));
    assert!(!twitter_ytdlp_analysis_is_placeholder(&text_post));
    assert!(!twitter_ytdlp_analysis_is_placeholder(&video_post));
}

#[test]
fn ytdlp_cookie_browser_spec_rejects_unknown_browsers() {
    assert_eq!(ytdlp_cookie_browser_spec("chrome").unwrap(), "chrome");
    assert_eq!(ytdlp_cookie_browser_spec("edge").unwrap(), "edge");
    assert!(ytdlp_cookie_browser_spec("unknown-browser").is_err());
}

#[test]
fn chromium_cookie_scan_prefers_latest_network_database() {
    let root =
        std::env::temp_dir().join(format!("mediadrop-opera-gx-cookie-test-{}", unique_stamp()));
    let default_cookie = root.join("Default").join("Network").join("Cookies");
    let profile_cookie = root.join("Profile 1").join("Network").join("Cookies");
    let ignored_cookie = root.join("Default").join("Cache").join("Cookies");

    fs::create_dir_all(default_cookie.parent().unwrap()).unwrap();
    fs::create_dir_all(profile_cookie.parent().unwrap()).unwrap();
    fs::create_dir_all(ignored_cookie.parent().unwrap()).unwrap();

    fs::write(&default_cookie, b"default").unwrap();
    fs::write(&ignored_cookie, b"ignored").unwrap();
    thread::sleep(Duration::from_millis(20));
    fs::write(&profile_cookie, b"profile one").unwrap();

    let candidates = chromium_cookie_candidates(&root);

    assert_eq!(candidates.len(), 2);
    assert_eq!(candidates[0].relative_path, "Profile 1/Network/Cookies");
    assert_eq!(candidates[1].relative_path, "Default/Network/Cookies");

    let _ = fs::remove_dir_all(root);
}

#[test]
fn instagram_cookie_host_filter_accepts_only_instagram_domains() {
    assert!(is_instagram_cookie_host("instagram.com"));
    assert!(is_instagram_cookie_host(".instagram.com"));
    assert!(is_instagram_cookie_host("www.instagram.com"));
    assert!(!is_instagram_cookie_host("notinstagram.com"));
    assert!(!is_instagram_cookie_host("instagram.com.evil.test"));
}

#[test]
fn instagram_cookie_profile_score_prefers_logged_in_session() {
    let guest = vec![NetscapeCookie {
        domain: ".instagram.com".to_string(),
        include_subdomains: true,
        path: "/".to_string(),
        secure: true,
        expires: 0,
        name: "csrftoken".to_string(),
        value: "token".to_string(),
    }];
    let logged_in = vec![
        NetscapeCookie {
            domain: ".instagram.com".to_string(),
            include_subdomains: true,
            path: "/".to_string(),
            secure: true,
            expires: 0,
            name: "sessionid".to_string(),
            value: "session".to_string(),
        },
        NetscapeCookie {
            domain: ".instagram.com".to_string(),
            include_subdomains: true,
            path: "/".to_string(),
            secure: true,
            expires: 0,
            name: "ds_user_id".to_string(),
            value: "42".to_string(),
        },
    ];

    assert!(cookie_profile_score(&logged_in) > cookie_profile_score(&guest));
    assert!(cookie_jar_has_login_session(&logged_in));
    assert!(!cookie_jar_has_login_session(&guest));
}

#[test]
fn netscape_cookie_jar_uses_tab_format_without_leaking_meta() {
    let text = cookie_jar_to_netscape(&[NetscapeCookie {
        domain: ".instagram.com".to_string(),
        include_subdomains: true,
        path: "/".to_string(),
        secure: true,
        expires: 1_900_000_000,
        name: "sessionid".to_string(),
        value: "abc123".to_string(),
    }]);

    assert!(text.contains("# Netscape HTTP Cookie File"));
    assert!(text.contains(".instagram.com\tTRUE\t/\tTRUE\t1900000000\tsessionid\tabc123"));
    assert!(!text.contains("profileLabel"));
    assert!(!text.contains("browserId"));
}

#[test]
fn social_default_format_selector_prefers_h264_mp4() {
    let selector = social_format_selector("best[ext=mp4]/bestvideo+bestaudio/best");

    assert!(selector.contains("[vcodec^=avc1]"));
    assert!(selector.contains("[vcodec^=h264]"));
    assert!(selector.ends_with("/best[ext=mp4]/best"));
}

#[test]
fn pretty_filename_preserves_turkish_and_spaces() {
    let name =
        pretty_output_filename_part("Çağrı'nın İstanbul videosu - 4K deneme", "MediaDrop Video");

    assert_eq!(name, "Çağrının İstanbul videosu - 4K deneme");
}

#[test]
fn pretty_filename_removes_windows_and_spoofing_chars() {
    let name = pretty_output_filename_part("CON <bad>|𝕬𝖇\u{202E} dosya???", "MediaDrop Video");

    assert_eq!(name, "bad dosya");
    assert_eq!(
        pretty_output_filename_part("CON", "MediaDrop Video"),
        "MediaDrop Video"
    );
}

#[test]
fn social_titles_are_shortened() {
    let title = pretty_media_title(
        "instagram",
        Some("Bugün İstanbul içinde çok güzel bir yürüyüş videosu çektim"),
    );

    assert_eq!(title, "Bugün İstanbul içinde çok güzel bir");
}

#[test]
fn twitter_post_card_png_validation_accepts_png_data_urls() {
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    png.extend_from_slice(&[0; 24]);
    let encoded = general_purpose::STANDARD.encode(&png);
    let data_url = format!("data:image/png;base64,{}", encoded);

    assert_eq!(decode_twitter_post_card_png(&data_url).unwrap(), png);
    assert!(decode_twitter_post_card_png("not-a-png").is_err());
}

#[test]
fn twitter_avatar_helpers_accept_only_safe_twitter_images() {
    assert!(twitter_avatar_host_allowed("pbs.twimg.com"));
    assert!(twitter_avatar_host_allowed("sub.twimg.com"));
    assert!(!twitter_avatar_host_allowed("twitter.com"));
    assert!(!twitter_avatar_host_allowed("x.com"));
    assert!(!twitter_avatar_host_allowed("localhost"));
    assert!(!twitter_avatar_host_allowed("127.0.0.1"));
    assert!(!twitter_avatar_host_allowed("example.com"));
    assert!(twitter_avatar_url_allowed(
        &reqwest::Url::parse("https://pbs.twimg.com/profile.jpg").unwrap()
    ));
    assert!(!twitter_avatar_url_allowed(
        &reqwest::Url::parse("http://pbs.twimg.com/profile.jpg").unwrap()
    ));
    assert!(!twitter_avatar_url_allowed(
        &reqwest::Url::parse("ftp://pbs.twimg.com/profile.jpg").unwrap()
    ));
    assert!(!twitter_avatar_url_allowed(
        &reqwest::Url::parse("file:///C:/profile.jpg").unwrap()
    ));
    assert!(!twitter_avatar_url_allowed(
        &reqwest::Url::parse("https://127.0.0.1/profile.jpg").unwrap()
    ));
    assert!(!twitter_avatar_url_allowed(
        &reqwest::Url::parse("https://0.0.0.0/profile.jpg").unwrap()
    ));
    assert!(!twitter_avatar_url_allowed(
        &reqwest::Url::parse("https://10.0.0.1/profile.jpg").unwrap()
    ));
    assert!(!twitter_avatar_url_allowed(
        &reqwest::Url::parse("https://192.168.1.20/profile.jpg").unwrap()
    ));
    assert!(!twitter_avatar_url_allowed(
        &reqwest::Url::parse("https://[::1]/profile.jpg").unwrap()
    ));
    assert_eq!(
        avatar_mime_from_text("image/jpeg; charset=binary"),
        Some("image/jpeg")
    );
    assert_eq!(
        avatar_mime_from_url_path("/profile/avatar.webp"),
        Some("image/webp")
    );
    assert_eq!(
        avatar_mime_from_url(
            &reqwest::Url::parse("https://pbs.twimg.com/media/example?format=jpg&name=small")
                .unwrap()
        ),
        Some("image/jpeg")
    );
    assert!(avatar_bytes_match_mime(
        b"\x89PNG\r\n\x1a\nabc",
        "image/png"
    ));
    assert!(!avatar_bytes_match_mime(b"not-png", "image/png"));
}

#[test]
fn twitter_avatar_body_reader_enforces_limit() {
    let bytes = read_limited_body(std::io::Cursor::new(vec![1, 2, 3, 4]), 4)
        .expect("body under limit should read");

    assert_eq!(bytes, vec![1, 2, 3, 4]);
    assert!(read_limited_body(std::io::Cursor::new(vec![1, 2, 3, 4, 5]), 4).is_err());
}

#[test]
fn twitter_post_card_layout_validation_normalizes_even_values() {
    let layout = validate_twitter_post_card_layout(TwitterPostCardLayout {
        output_width: 1081,
        output_height: 901,
        video_x: 121,
        video_y: 281,
        video_width: 841,
        video_height: 473,
    })
    .expect("layout should be valid");

    assert_eq!(layout.output_width, 1080);
    assert_eq!(layout.output_height, 900);
    assert_eq!(layout.video_x, 120);
    assert_eq!(layout.video_y, 280);
    assert_eq!(layout.video_width, 840);
    assert_eq!(layout.video_height, 472);

    assert!(validate_twitter_post_card_layout(TwitterPostCardLayout {
        output_width: 1080,
        output_height: 900,
        video_x: 600,
        video_y: 300,
        video_width: 840,
        video_height: 472,
    })
    .is_err());
}

#[cfg(target_os = "windows")]
#[test]
fn sqlite_cookie_snapshot_uses_safe_bound_readers_without_creating_missing_files() {
    let test_dir =
        std::env::temp_dir().join(format!("mediadrop-sqlite-snapshot-test-{}", unique_stamp()));
    fs::create_dir_all(&test_dir).expect("test directory should be created");
    let source = test_dir.join("Cookies");

    {
        let connection = rusqlite::Connection::open(&source).expect("source database should open");
        connection
            .execute_batch(
                "CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT); \
                 CREATE TABLE cookies ( \
                   host_key TEXT, name TEXT, value TEXT, encrypted_value BLOB, \
                   path TEXT, expires_utc INTEGER, is_secure INTEGER \
                 ); \
                 CREATE TABLE moz_cookies ( \
                   host TEXT, name TEXT, value TEXT, path TEXT, expiry INTEGER, isSecure INTEGER \
                 );",
            )
            .expect("fixture schema should be created");
        connection
            .execute(
                "INSERT INTO meta (key, value) VALUES (?1, ?2)",
                rusqlite::params!["version", "24"],
            )
            .expect("meta row should be inserted");
        connection
            .execute(
                "INSERT INTO cookies \
                 (host_key, name, value, encrypted_value, path, expires_utc, is_secure) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    ".instagram.com",
                    "sessionid",
                    "fixture-session",
                    Vec::<u8>::new(),
                    "/",
                    13_300_000_000_000_000_i64,
                    1_i64
                ],
            )
            .expect("Chromium row should be inserted");
        connection
            .execute(
                "INSERT INTO cookies \
                 (host_key, name, value, encrypted_value, path, expires_utc, is_secure) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    ".example.com",
                    "ignored",
                    "not-instagram",
                    Vec::<u8>::new(),
                    "/",
                    0_i64,
                    0_i64
                ],
            )
            .expect("non-Instagram Chromium row should be inserted");
        connection
            .execute(
                "INSERT INTO moz_cookies \
                 (host, name, value, path, expiry, isSecure) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    "instagram.com",
                    "ds_user_id",
                    "42",
                    "/",
                    2_000_000_000_i64,
                    1_i64
                ],
            )
            .expect("Firefox row should be inserted");
    }

    let snapshot = snapshot_sqlite_database(&source, "safe-test")
        .expect("a consistent SQLite snapshot should be created");
    assert_ne!(snapshot, source);
    assert_eq!(sqlite_win::read_meta_version(&snapshot).unwrap(), 24);

    let chromium = sqlite_win::read_chromium_cookie_rows(&snapshot).unwrap();
    assert_eq!(chromium.len(), 1);
    assert_eq!(chromium[0].host_key, ".instagram.com");
    assert_eq!(chromium[0].name, "sessionid");
    assert!(chromium[0].is_secure);

    let firefox = sqlite_win::read_firefox_cookie_rows(&snapshot).unwrap();
    assert_eq!(firefox.len(), 1);
    assert_eq!(firefox[0].host, "instagram.com");
    assert_eq!(firefox[0].name, "ds_user_id");
    assert!(firefox[0].is_secure);

    let missing = test_dir.join("must-not-be-created.sqlite");
    assert!(sqlite_win::read_meta_version(&missing).is_err());
    assert!(
        !missing.exists(),
        "read-only open must never create a database"
    );

    cleanup_sqlite_snapshot(&snapshot);
    let _ = fs::remove_dir_all(test_dir);
}
