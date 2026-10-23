//! Checks that the HTTP client factory is called with the right
//! `HttpClientPurpose`. The factory records each purpose it's called with,
//! and the tests check the recording.

use std::sync::{Arc, Mutex};

use steamkit::steam::steam_client::configuration::i_steam_configuration_builder::ISteamConfigurationBuilder;
use steamkit::steam::steam_client::configuration::steam_configuration::{
    default_http_client_factory, HttpClientPurpose, SteamConfiguration,
};

/// Builds a configuration whose http client factory records every
/// `HttpClientPurpose` it is invoked with, and returns the recording alongside
/// the configuration.
fn purpose_recording_configuration() -> (SteamConfiguration, Arc<Mutex<Vec<HttpClientPurpose>>>) {
    let recorded: Arc<Mutex<Vec<HttpClientPurpose>>> = Arc::new(Mutex::new(Vec::new()));
    let recorder = recorded.clone();

    let configuration = SteamConfiguration::create(|b| {
        b.with_http_client_factory(Arc::new(move |purpose: HttpClientPurpose| {
            recorder
                .lock()
                .expect("the purpose recorder lock must not be poisoned")
                .push(purpose);
            default_http_client_factory()(purpose)
        }));
    });

    (configuration, recorded)
}

#[test]
fn web_api_purpose_receives_correct_parameter() {
    let (configuration, recorded) = purpose_recording_configuration();

    let _client = (configuration.http_client_factory())(HttpClientPurpose::WebAPI);

    assert_eq!(
        recorded
            .lock()
            .expect("the purpose recorder lock must not be poisoned")
            .as_slice(),
        &[HttpClientPurpose::WebAPI]
    );
}

#[test]
fn cm_web_socket_purpose_receives_correct_parameter() {
    let (configuration, recorded) = purpose_recording_configuration();

    let _client = (configuration.http_client_factory())(HttpClientPurpose::CMWebSocket);

    assert_eq!(
        recorded
            .lock()
            .expect("the purpose recorder lock must not be poisoned")
            .as_slice(),
        &[HttpClientPurpose::CMWebSocket]
    );
}

#[test]
fn cdn_purpose_receives_correct_parameter() {
    let (configuration, recorded) = purpose_recording_configuration();

    let _client = (configuration.http_client_factory())(HttpClientPurpose::CDN);

    assert_eq!(
        recorded
            .lock()
            .expect("the purpose recorder lock must not be poisoned")
            .as_slice(),
        &[HttpClientPurpose::CDN]
    );
}

#[test]
fn different_purposes_receive_different_configurations() {
    let (configuration, recorded) = purpose_recording_configuration();

    let _web_api_client = (configuration.http_client_factory())(HttpClientPurpose::WebAPI);
    let _cdn_client = (configuration.http_client_factory())(HttpClientPurpose::CDN);

    assert_eq!(
        recorded
            .lock()
            .expect("the purpose recorder lock must not be poisoned")
            .as_slice(),
        &[HttpClientPurpose::WebAPI, HttpClientPurpose::CDN]
    );
}
