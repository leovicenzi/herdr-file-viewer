use super::*;

fn speak(id: u64) -> SpeechCommand {
    SpeechCommand::Speak {
        id,
        document: 1,
        offset: id as usize,
        paused: true,
        chunks: Vec::new(),
        speed: 1.0,
    }
}

#[test]
fn rapid_seeks_coalesce_without_blocking_on_an_unread_wakeup() {
    let (queue, _rx) = CommandQueue::new();
    for id in 1..=10000 {
        queue.send(speak(id)).unwrap();
    }
    let pending = queue.pending.lock().unwrap();
    assert_eq!(pending.len(), 1);
    assert!(matches!(
        pending.front(),
        Some(SpeechCommand::Speak {
            id: 10000,
            paused: true,
            ..
        })
    ));
}

#[test]
fn control_updates_are_bounded_and_new_seek_supersedes_old_controls() {
    let (queue, _rx) = CommandQueue::new();
    queue.send(speak(1)).unwrap();
    for _ in 0..10000 {
        queue
            .send(SpeechCommand::Pause {
                id: 1,
                paused: true,
            })
            .unwrap();
        queue
            .send(SpeechCommand::Speed { id: 1, speed: 1.5 })
            .unwrap();
    }
    assert_eq!(queue.pending.lock().unwrap().len(), 3);
    queue.send(speak(2)).unwrap();
    assert_eq!(queue.pending.lock().unwrap().len(), 1);
}

#[test]
fn stop_keeps_undelivered_speak_so_helper_knows_the_stop_id() {
    let (queue, _rx) = CommandQueue::new();
    queue.send(speak(2)).unwrap();
    queue
        .send(SpeechCommand::Pause {
            id: 2,
            paused: false,
        })
        .unwrap();
    queue.send(SpeechCommand::Stop { id: 2 }).unwrap();
    let pending = queue.pending.lock().unwrap();
    assert_eq!(pending.len(), 2);
    assert!(matches!(pending[0], SpeechCommand::Speak { id: 2, .. }));
    assert!(matches!(pending[1], SpeechCommand::Stop { id: 2 }));
}

#[cfg(unix)]
#[test]
fn release_reaps_the_helper_silently_and_the_next_send_starts_another() {
    let mut speech = CommandSpeech::new(vec!["sh".into(), "-c".into(), "cat > /dev/null".into()]);
    speech.send(speak(1)).unwrap();
    let pid = speech.child.as_ref().unwrap().id();
    speech.release();
    assert!(speech.child.is_none() && speech.tx.is_none() && speech.rx.is_none());
    // Reaped by release, so the pid is gone rather than a zombie awaiting wait().
    if cfg!(target_os = "linux") {
        assert!(!std::path::Path::new(&format!("/proc/{pid}")).exists());
    }
    assert!(
        speech.poll().is_empty(),
        "a released helper is not an error"
    );
    speech.send(speak(2)).unwrap();
    assert!(speech.child.is_some());
}
