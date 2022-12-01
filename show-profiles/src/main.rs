#[cfg(windows)]
use windows::core::{Result, GUID};
#[cfg(windows)]
use windows::Networking::Connectivity::{
    ConnectionProfile, ConnectionProfileFilter, NetworkInformation, NetworkStatusChangedEventHandler,
};
#[cfg(windows)]
use windows::Networking::HostNameType;

#[cfg(windows)]
#[tokio::main]
async fn main() -> Result<()> {
    let mut ipaddrs = Vec::new();

    for hostname in NetworkInformation::GetHostNames()? {
        match hostname.Type()? {
            HostNameType::Ipv4 | HostNameType::Ipv6 => {
                ipaddrs.push(hostname);
            }
            _ => {}
        }
    }

    for ipaddr in &ipaddrs {
        let adapter = ipaddr.IPInformation()?.NetworkAdapter()?;
        let profile = find_connected_profile(adapter.NetworkAdapterId()?).await?;
        if let Some(profile) = profile {
            println!(
                "{}: {}",
                ipaddr.ToString()?,
                profile.ProfileName()?
            );
            println!("\tCostType: {:?}", profile.GetConnectionCost()?.NetworkCostType()?);
        }
    }
    let handler = NetworkStatusChangedEventHandler::new(|v| {
        println!("{:?}", v);
        if let Some(detail) = v.as_ref() {
            println!("{:?}", detail);
        }
        println!("changed");
        Ok(())
    });
    let token = NetworkInformation::NetworkStatusChanged(&handler)?;

    use std::time::Duration;

    tokio::time::sleep(Duration::MAX).await;

    /*
    let adapter = hostname.IPInformation()?.NetworkAdapter()?;
    println!("{}: ", hostname.ToString()?);
    println!("\tAdapter: {:?}", adapter.NetworkAdapterId()?);
    println!("\t\tType: {:?}", adapter.IanaInterfaceType()?);
    */

    Ok(())
}

#[cfg(windows)]
async fn find_connected_profile(guid: GUID) -> Result<Option<ConnectionProfile>> {
    let filter = ConnectionProfileFilter::new()?;
    filter.SetIsConnected(true).unwrap();
    for profile in NetworkInformation::FindConnectionProfilesAsync(&filter)?.await? {
        if profile.NetworkAdapter()?.NetworkAdapterId()? == guid {
            return Ok(Some(profile));
        }
    }
    Ok(None)
}

#[cfg(unix)]
fn main() {
    println!("Not supported on unix");
}
