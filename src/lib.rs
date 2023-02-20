//! rings-node
//! ===============
//! [![rings-node](https://github.com/RingsNetwork/rings-node/actions/workflows/rings-node.yml/badge.svg)](https://github.com/RingsNetwork/rings-node/actions/workflows/rings-node.yml)
//! [![cargo](https://img.shields.io/crates/v/rings-node.svg)](https://crates.io/crates/rings-node)
//! [![docs](https://docs.rs/rings-node/badge.svg)](https://docs.rs/rings-node/0.1.0/rings_node/)
//!
//! Rings Network is a decentralized peer-to-peer network that has built a more decentralized, anonymous and privacy-oriented data sovereignty network based on Chord computation. Rings Network is built with a communication layer based on WebAssembly, which allows it to run directly in the browser and connect directly between browsers via the webRTC protocol, further solving the problem of the modern Internet being controlled by centralized entities. Rings Network supports the use of elliptic curves as DIDs for proofs, and its stability is sufficient to support a large number of nodes and efficient lookups, making the connection and data exchange between sovereign entities more secure, efficient, and direct. This paper introduces the four-layer architecture of Rings Network and analyzes Rings Network from various aspects, including network, traffic, orderer, security, and shows the unlimited possibilities of Rings Network itself and as an application platform.
//!
//! ### ICE Scheme:

//! 1. Peer A:
//! {
//!     create offer,
//!     set it as local description
//! } -> Send Offer to Peer B

//! 2. Peer B: {
//!   set receiveed offer as remote description
//!   create answer
//!   set it as local description
//!   Send Answer to Peer A
//! }

//! 3. Peer A: {
//!    Set receiveed answer as remote description
//! }

//! ### Keywords

//! * Candidate

//!     - A CANDIDATE is a transport address -- a combination of IP address and port for a particular transport protocol (with only UDP specified here).

//!     - If an agent is multihomed, it obtains a candidate from each IP address.

//!     - The agent uses STUN or TURN to obtain additional candidates. These come in two flavors: translated addresses on the public side of a NAT (SERVER REFLEXIVE CANDIDATES) and addresses on TURN servers (RELAYED CANDIDATES).

//! ```text
//!                  To Internet

//!                      |
//!                      |
//!                      |  /------------  Relayed
//!                  Y:y | /               Address
//!                  +--------+
//!                  |        |
//!                  |  TURN  |
//!                  | Server |
//!                  |        |
//!                  +--------+
//!                      |
//!                      |
//!                      | /------------  Server
//!               X1':x1'|/               Reflexive
//!                +------------+         Address
//!                |    NAT     |
//!                +------------+
//!                      |
//!                      | /------------  Local
//!                  X:x |/               Address
//!                  +--------+
//!                  |        |
//!                  | Agent  |
//!                  |        |
//!                  +--------+

//!                      Figure 2: Candidate Relationships

//! ```

//! * Channel

//! In the WebRTC framework, communication between the parties consists of media (for example, audio and video) and non-media data.

//! Non-media data is handled by using the Stream Control Transmission Protocol (SCTP) encapsulated in DTLS.

//! ```text
//!                                +----------+
//!                                |   SCTP   |
//!                                +----------+
//!                                |   DTLS   |
//!                                +----------+
//!                                | ICE/UDP  |
//!                                +----------+

//! ```

//! The encapsulation of SCTP over DTLS (see RFC8261) over ICE/UDP (see RFC8445) provides a NAT traversal solution together with confidentiality, source authentication, and integrity-protected transfers.

//!  The layering of protocols for WebRTC is shown as:

//! ```text
//!                                  +------+------+------+
//!                                  | DCEP | UTF-8|Binary|
//!                                  |      | Data | Data |
//!                                  +------+------+------+
//!                                  |        SCTP        |
//!                    +----------------------------------+
//!                    | STUN | SRTP |        DTLS        |
//!                    +----------------------------------+
//!                    |                ICE               |
//!                    +----------------------------------+
//!                    | UDP1 | UDP2 | UDP3 | ...         |
//!                    +----------------------------------+
//! ```

//! ### Architecture

//! ```text
//! +-----------------------------------------------------------------------------------------+
//! |                                         RINGS                                           |
//! +-----------------------------------------------------------------------------------------+
//! |   Encrypted IM / Secret Sharing Storage / Distributed Content / Secret Data Exchange    |
//! +-----------------------------------------------------------------------------------------+
//! |                  SSSS                  |  Perdson Commitment/zkPod/ Secret Sharing      |
//! +-----------------------------------------------------------------------------------------+
//! |                     |       dDNS       |                 Sigma Protocol                 |
//! +      K-V Storage    +------------------+------------------------------------------------+
//! |                     |  Peer LOOKUP     |          MSRP        |  End-to-End Encryption  |
//! +----------------------------------------+------------------------------------------------+
//! |            Peer-to-Peer Network        |                                                |
//! |----------------------------------------+               DID / Resource ID                |
//! |                 Chord DHT              |                                                |
//! +----------------------------------------+------------------------------------------------+
//! |                Trickle SDP             |        ElGamal        | Persistence Storage    |
//! +----------------------------------------+-----------------------+------------------------+
//! |            STUN  | SDP  | ICE          |  Crosschain Binding   | Smart Contract Binding |
//! +----------------------------------------+------------------------------------------------+
//! |             SCTP | UDP | TCP           |             Finate Pubkey Group                |
//! +----------------------------------------+------------------------------------------------+
//! |   WASM Runtime    |                    |                                                |
//! +-------------------|  Operation System  |                   ECDSA                        |
//! |     Browser       |                    |                                                |
//! +-----------------------------------------------------------------------------------------+
//! ```

pub mod backend;
#[cfg(feature = "browser")]
pub mod browser;
#[cfg(feature = "node")]
pub mod cli;
#[cfg(not(feature = "browser"))]
pub mod config;
pub mod consts;
pub mod error;
pub mod jsonrpc;
pub mod jsonrpc_client;
pub mod logging;
pub mod measure;
pub mod prelude;
pub mod processor;
pub mod seed;
#[cfg(feature = "node")]
pub mod service;
pub mod util;
