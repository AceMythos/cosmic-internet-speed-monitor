//! Standalone probe for the D-Bus calls the applet makes, so failures can be
//! attributed to a specific step instead of guessed at.
//!
//! Run with: cargo run --example probe

fn main() {
    let rt = match tokio::runtime::Runtime::new() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("STEP 1 FAIL: runtime: {e}");
            return;
        }
    };

    rt.block_on(async {
        // STEP 1: session bus
        let conn = match zbus::Connection::session().await {
            Ok(c) => c,
            Err(e) => {
                eprintln!("STEP 1 FAIL: session bus: {e}");
                return;
            }
        };
        println!("STEP 1 OK: session bus connected");

        // STEP 2: list devices
        let reply = match conn
            .call_method(
                Some("org.kde.kdeconnect"),
                "/modules/kdeconnect",
                Some("org.kde.kdeconnect.daemon"),
                "devices",
                &(false, false),
            )
            .await
        {
            Ok(r) => r,
            Err(e) => {
                eprintln!("STEP 2 FAIL: devices call: {e}");
                return;
            }
        };
        let devices: Result<Vec<String>, _> = reply.body().deserialize();
        let devices = match devices {
            Ok(d) => {
                println!("STEP 2 OK: {} device(s): {d:?}", d.len());
                d
            }
            Err(e) => {
                eprintln!("STEP 2 FAIL: deserialize: {e}");
                return;
            }
        };
        let Some(id) = devices.into_iter().next() else {
            eprintln!("STEP 2b FAIL: no paired device");
            return;
        };

        // STEP 3: read the property
        let path = format!("/modules/kdeconnect/devices/{id}/connectivity_report");
        println!("STEP 3: calling Get on {path}");
        let reply = match conn
            .call_method(
                Some("org.kde.kdeconnect"),
                path.as_str(),
                Some("org.freedesktop.DBus.Properties"),
                "Get",
                &(
                    "org.kde.kdeconnect.device.connectivity_report",
                    "cellularNetworkType",
                ),
            )
            .await
        {
            Ok(r) => r,
            Err(e) => {
                eprintln!("STEP 3 FAIL: Get call: {e}");
                return;
            }
        };

        // STEP 4: Properties.Get returns a VARIANT. In zvariant a variant is
// Value::Value(inner), so unwrap that before reading the string.
        let body = reply.body();
        let value: zbus::zvariant::Value = match body.deserialize() {
            Ok(v) => v,
            Err(e) => {
                eprintln!("STEP 4 FAIL: deserialize to Value: {e}");
                return;
            }
        };
        let inner = match value {
            zbus::zvariant::Value::Value(inner) => *inner,
            other => other,
        };
        match inner.downcast_ref::<String>() {
            Ok(s) => println!("STEP 4 OK: value = {s:?}"),
            Err(e) => eprintln!("STEP 4 FAIL: downcast to String: {e}"),
        }
    });
}