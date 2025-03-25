#![no_std]
use core::ops::DerefMut;

use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_sync::once_lock::OnceLock;
use embedded_services::power::policy::device::Device;
use embedded_services::power::policy::{action, policy, *};
use embedded_services::{comms, error, info};

pub mod consumer;

struct InternalState {
    /// Current consumer state, if any
    current_consumer_state: Option<consumer::State>,
}

impl InternalState {
    fn new() -> Self {
        Self {
            current_consumer_state: None,
        }
    }
}

/// Power policy state
pub struct PowerPolicy {
    /// Power policy context
    context: policy::ContextToken,
    /// State
    state: Mutex<NoopRawMutex, InternalState>,
    /// Comms endpoint
    tp: comms::Endpoint,
}

impl PowerPolicy {
    /// Create a new power policy
    pub fn create() -> Option<Self> {
        Some(Self {
            context: policy::ContextToken::create()?,
            state: Mutex::new(InternalState::new()),
            tp: comms::Endpoint::uninit(comms::EndpointID::Internal(comms::Internal::Power)),
        })
    }

    async fn process_notify_attach(&self) -> Result<(), Error> {
        self.context.send_response(Ok(policy::ResponseData::Complete)).await;
        Ok(())
    }

    async fn process_notify_detach(&self) -> Result<(), Error> {
        self.context.send_response(Ok(policy::ResponseData::Complete)).await;
        self.update_current_consumer().await?;
        Ok(())
    }

    async fn process_notify_consumer_power_capability(&self) -> Result<(), Error> {
        self.context.send_response(Ok(policy::ResponseData::Complete)).await;
        self.update_current_consumer().await?;
        Ok(())
    }

    async fn process_request_provider_power_capabilities(&self) -> Result<(), Error> {
        self.context.send_response(Ok(policy::ResponseData::Complete)).await;
        Ok(())
    }

    async fn process_notify_disconnect(&self) -> Result<(), Error> {
        self.context.send_response(Ok(policy::ResponseData::Complete)).await;
        self.update_current_consumer().await?;
        Ok(())
    }

    /// Send a notification with the comms service
    async fn comms_notify(&self, message: CommsMessage) {
        let _ = self
            .tp
            .send(comms::EndpointID::Internal(comms::Internal::Battery), &message)
            .await;
    }

    pub async fn process_request(&self) -> Result<(), Error> {
        let request = self.context.wait_request().await;
        let device = self.context.get_device(request.id).await?;

        match request.data {
            policy::RequestData::NotifyAttached => {
                info!("Received notify attached from device {}", device.id().0);
                self.process_notify_attach().await
            }
            policy::RequestData::NotifyDetached => {
                info!("Received notify detached from device {}", device.id().0);
                self.process_notify_detach().await
            }
            policy::RequestData::NotifyConsumerCapability(capability) => {
                info!(
                    "Received notify consumer capability from device {}: {:#?}",
                    device.id().0,
                    capability
                );
                self.process_notify_consumer_power_capability().await
            }
            policy::RequestData::RequestProviderCapability(capability) => {
                info!(
                    "Received request provider capability from device {}: {:#?}",
                    device.id().0,
                    capability
                );
                self.process_request_provider_power_capabilities().await
            }
            policy::RequestData::NotifyDisconnect => {
                info!("Received notify disconnect from device {}", device.id().0);
                self.process_notify_disconnect().await
            }
        }
    }
}

impl comms::MailboxDelegate for PowerPolicy {
    fn receive(&self, _message: &comms::Message) {}
}

#[embassy_executor::task]
pub async fn task() {
    info!("Starting power policy task");
    static POLICY: OnceLock<PowerPolicy> = OnceLock::new();
    let policy = POLICY.get_or_init(|| PowerPolicy::create().expect("Power policy singleton already initialized"));

    if comms::register_endpoint(policy, &policy.tp).await.is_err() {
        error!("Failed to register power policy endpoint");
        return;
    }

    loop {
        if let Err(e) = policy.process_request().await {
            error!("Error processing request: {:?}", e);
        }
    }
}
