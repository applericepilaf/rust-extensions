/*
   Copyright The containerd Authors.

   Licensed under the Apache License, Version 2.0 (the "License");
   you may not use this file except in compliance with the License.
   You may obtain a copy of the License at

       http://www.apache.org/licenses/LICENSE-2.0

   Unless required by applicable law or agreed to in writing, software
   distributed under the License is distributed on an "AS IS" BASIS,
   WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
   See the License for the specific language governing permissions and
   limitations under the License.
*/

use std::process::Output;

use async_trait::async_trait;
use log::error;
use time::OffsetDateTime;
use tokio::sync::oneshot::{Receiver, Sender};

/// A trait for spawning and waiting for a process.
///
/// The design is different from Go's, because if you return a `Sender` in [ProcessMonitor::start()]
/// and want to use it in [ProcessMonitor::wait()], then start and wait cannot be executed
/// concurrently. Alternatively, let the caller to prepare the communication channel for
/// [ProcessMonitor::start()] and [ProcessMonitor::wait()] so they could be executed concurrently.
#[async_trait]
pub trait ProcessMonitor {
    /// Spawn a process and return its output.
    ///
    /// In order to capture the output/error, it is necessary for the caller to create new pipes
    /// between parent and child.
    /// Use [tokio::process::Command::stdout(Stdio::piped())](https://docs.rs/tokio/1.16.1/tokio/process/struct.Command.html#method.stdout)
    /// and/or [tokio::process::Command::stderr(Stdio::piped())](https://docs.rs/tokio/1.16.1/tokio/process/struct.Command.html#method.stderr)
    /// respectively, when creating the [Command](https://docs.rs/tokio/1.16.1/tokio/process/struct.Command.html#).
    async fn start(
        &self,
        mut cmd: tokio::process::Command,
        tx: Sender<Exit>,
    ) -> std::io::Result<Output> {
        let chi = cmd.spawn()?;
        // Safe to expect() because wait() hasn't been called yet, dependence on tokio interanl
        // implementation details.
        let pid = chi
            .id()
            .expect("failed to take pid of the container process.");
        let out = chi.wait_with_output().await?;
        let ts = OffsetDateTime::now_utc();
        match tx.send(Exit {
            ts,
            pid,
            status: out.status.code().unwrap(),
        }) {
            Ok(_) => Ok(out),
            Err(e) => {
                error!("command {:?} exited but receiver dropped.", cmd);
                error!("couldn't send messages: {:?}", e);
                Err(std::io::ErrorKind::ConnectionRefused.into())
            }
        }
    }

    /// Wait for the spawned process to exit and return the exit status.
    async fn wait(&self, rx: Receiver<Exit>) -> std::io::Result<Exit> {
        rx.await.map_err(|_| {
            error!("sender dropped.");
            std::io::ErrorKind::BrokenPipe.into()
        })
    }
}

/// A default implementation of [ProcessMonitor].
#[derive(Debug, Clone, Default)]
pub struct DefaultMonitor {}

impl ProcessMonitor for DefaultMonitor {}

impl DefaultMonitor {
    pub const fn new() -> Self {
        Self {}
    }
}

/// Process exit status returned by [ProcessMonitor::wait()].
#[derive(Debug)]
pub struct Exit {
    pub ts: OffsetDateTime,
    pub pid: u32,
    pub status: i32,
}
