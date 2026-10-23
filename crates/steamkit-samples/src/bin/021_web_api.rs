//! Shows how to use the WebAPI utilities to talk to the Steam Web APIs.
//!
//! The Steam Web APIs are a set of "interfaces" with methods, a bit like
//! classes in OO languages, and the API for calling them works the same way.

use std::time::Duration;

use anyhow::Result;
use steamkit::steam::webapi::web_api::{web_api_args, Interface, WebAPI, WebApiValue};
use steamkit::types::key_value::KeyValue;

/// Prints the title of every news item of a response.
fn print_news(news_items: &KeyValue) {
    for news in news_items.children() {
        println!("News: {}", news.get("title").as_string().unwrap_or_default());
    }
}

/// Queries the Steam Web API.
#[tokio::main]
async fn main() -> Result<()> {
    // in order to interact with the Web APIs, you must first acquire an interface
    // for a certain API
    let steam_news = WebAPI::get_interface("ISteamNews", "");

    // the ISteamNews WebAPI has only 1 function: GetNewsForApp, so we'll be
    // using that
    //
    // call the function by name with its version, and pass the parameters as
    // name/value pairs
    let kv_news = steam_news
        .call(
            "GetNewsForApp",
            1,
            &web_api_args([("appid", WebApiValue::from(440u32))]),
        )
        .await?;

    // the return of every WebAPI call is a KeyValue that contains the result
    // data
    //
    // for this example we'll iterate the results and display the title
    print_news(kv_news.get("newsitems").get("newsitem"));

    // for functions with multiple versions, pass the version you want
    let kv_news = steam_news
        .call(
            "GetNewsForApp",
            2,
            &web_api_args([("appid", WebApiValue::from(570u32))]),
        )
        .await?;

    // notice that the output of this version differs from the first version
    print_news(kv_news.get("newsitems"));

    // note that the interface functions return an error when the API is
    // otherwise inaccessible (networking issues, server downtime, etc) and
    // these should be handled appropriately
    match steam_news
        .call(
            "GetNewsForApp",
            2,
            &web_api_args([
                ("appid", WebApiValue::from(730u32)),
                ("maxlength", WebApiValue::from(100u32)),
                ("count", WebApiValue::from(5u32)),
            ]),
        )
        .await
    {
        Ok(kv_news) => print_news(kv_news.get("newsitems").get("newsitem")),
        Err(error) => {
            println!("Unable to make GetNewsForApp API request: {error}");
        }
    }

    // for WebAPIs that require an API key, the key can be specified in
    // get_interface
    let steam_user_auth = WebAPI::get_interface("ISteamUserAuth", "APIKEYGOESHERE");

    // it may be worth setting a timeout for calls
    steam_user_auth.set_timeout(Duration::from_secs(5));

    // additionally, if the API you are using requires you to POST, use
    // call_with_method
    match steam_user_auth
        .call_with_method(
            reqwest::Method::POST,
            "AuthenticateUser",
            1,
            &web_api_args([("someParam", WebApiValue::from("someValue"))]),
        )
        .await
    {
        Ok(_) => {}
        Err(error) => {
            println!("Unable to make AuthenticateUser API Request: {error}");
        }
    }

    // parameters can also be passed as plain strings
    let steam_news: Interface = WebAPI::get_interface("ISteamNews", "");

    let news_args = web_api_args([("appid", WebApiValue::Text("440".to_string()))]);

    let results = steam_news.call("GetNewsForApp", 1, &news_args).await?;

    print_news(results.get("newsitems").get("newsitem"));

    Ok(())
}
