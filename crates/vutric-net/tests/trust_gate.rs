//! S1–S3 end to end: an unknown device is refused before its first message, admitted while the pairing window
//! is open, and two devices pair with the commit-then-reveal code; afterwards the default gate lets the paired
//! one in. Existing tests (`two_peers.rs`) cover only the old check code with no gate.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use vutric_core::Platform;
use vutric_net::trust::{self, Admission, PairMethod, TrustStore, Visibility};
use vutric_net::{NotAdmitted, Peer, Reach};

async fn pair_of(a: u8, b: u8) -> (Peer, Peer) {
    let desktop = Peer::bind([a; 32], "Studio PC", Platform::Windows, Reach::Lan)
        .await
        .expect("bind desktop");
    let phone = Peer::bind([b; 32], "Pixel 8", Platform::Android, Reach::Lan)
        .await
        .expect("bind phone");
    (desktop, phone)
}

type Gate = Arc<Mutex<(TrustStore, Visibility)>>;

fn gate_fn(g: &Gate) -> impl Fn(&vutric_core::EndpointId) -> Admission + Send + Sync + 'static {
    let g = Arc::clone(g);
    move |id| {
        let (store, vis) = &*g.lock().unwrap();
        trust::admit(store, *vis, id, trust::now())
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn unknown_refused_then_paired_by_code_then_trusted() {
    let (desktop, phone) = pair_of(11, 12).await;
    let ticket = desktop.ticket();
    let gate: Gate = Arc::new(Mutex::new((TrustStore::default(), Visibility::TrustedOnly)));

    // 1. Default: the unknown phone is closed before the handshake, and learns why.
    let accept = tokio::spawn({
        let (desktop, f) = (desktop.clone(), gate_fn(&gate));
        async move { desktop.accept_with(f).await }
    });
    let err = phone
        .connect(ticket.endpoint, &ticket.addrs)
        .await
        .expect_err("an unknown device must be refused");
    assert!(
        err.downcast_ref::<NotAdmitted>().is_some(),
        "refusal is typed: {err:#}"
    );
    accept.abort();

    // 2. The desktop opens the pairing window; the phone gets in as Unknown and both run the code exchange.
    gate.lock().unwrap().1 = Visibility::pairing(trust::now());
    let accept = tokio::spawn({
        let (desktop, f) = (desktop.clone(), gate_fn(&gate));
        async move {
            let (mut s, admission) = desktop.accept_with(f).await.expect("accept");
            assert_eq!(admission, Admission::Unknown);
            let code = s.pair_code().await.expect("listener code");
            let both = s.pair_finish(true).await.expect("finish");
            (code, both, s.remote_id(), s)
        }
    });
    let mut s = phone
        .connect(ticket.endpoint, &ticket.addrs)
        .await
        .expect("connect in pairing window");
    assert!(s.is_dialer());
    let phone_code = s.pair_code().await.expect("dialer code");
    assert!(s.pair_finish(true).await.expect("finish"));
    let (desktop_code, both, phone_id, _keep) =
        tokio::time::timeout(Duration::from_secs(10), accept)
            .await
            .expect("desktop finished")
            .expect("desktop task");
    assert!(both);
    assert_eq!(
        phone_code, desktop_code,
        "both screens show the same digits"
    );
    assert_eq!(phone_code.len(), 6);
    assert_eq!(phone_id, phone.id());
    {
        let mut g = gate.lock().unwrap();
        g.0.trust(
            phone_id,
            "Pixel 8",
            Some(Platform::Android),
            PairMethod::Code,
            trust::now(),
        );
        g.1 = Visibility::TrustedOnly;
    }
    s.close();

    // 3. Back to the default: the paired phone is let in as Trusted.
    let accept = tokio::spawn({
        let (desktop, f) = (desktop.clone(), gate_fn(&gate));
        async move { desktop.accept_with(f).await.expect("accept trusted").1 }
    });
    let s = phone
        .connect(ticket.endpoint, &ticket.addrs)
        .await
        .expect("trusted connect");
    assert_eq!(accept.await.unwrap(), Admission::Trusted);
    s.close();
    phone.close().await;
    desktop.close().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn one_side_declining_stores_nothing() {
    let (desktop, phone) = pair_of(21, 22).await;
    let ticket = desktop.ticket();
    let accept = tokio::spawn({
        let desktop = desktop.clone();
        async move {
            let (mut s, _) = desktop
                .accept_with(|_| Admission::Unknown)
                .await
                .expect("accept");
            s.pair_code().await.expect("code");
            let both = s.pair_finish(false).await.expect("finish");
            // A "no" usually ends the session: close gracefully, so the answer is not lost in the close.
            s.finish().await;
            both
        }
    });
    let mut s = phone
        .connect(ticket.endpoint, &ticket.addrs)
        .await
        .expect("connect");
    s.pair_code().await.expect("code");
    assert!(!s.pair_finish(true).await.expect("finish"));
    assert!(!accept.await.unwrap());
    s.close();
    phone.close().await;
    desktop.close().await;
}
