# ave.rs

A simplified rewrite of [Avers][avers] in Rust, providing operational transformation capabilities with Firestore backend integration.

## Overview

This project is a Rust-based backend that implements operational transformation (OT) for collaborative applications, specifically designed to work with multiple climbing gym management systems through a single API.

## Motivation

- **Database Migration**: Moving from RethinkDB to Firestore for simplified cloud deployment
- **Multi-tenant Architecture**: Supporting multiple gyms through a single API instance
- **Learning Opportunity**: Gaining experience with Axum backends in Rust

## Architecture

The codebase is split into two main components:

- **OT Crate**: Simplified operational transformation implementation
- **Backend**: Axum-based API server with Firestore integration

## Current Limitations

### Implementation Shortcuts

- Object types are hardcoded (boulders and accounts), preventing OT crate reusability
- Some unused features from the original backend were removed (e.g., releases)

### Technical Constraints

- **Firestore Listeners**: Limited client support (polling might be more suitable for larger scale)
- **OT Operations**: Supports the same subset of operations as the original Avers implementation

## Why a Custom OT Implementation?

While excellent Rust OT implementations exist, this project uses a custom implementation because:

- **Focused Scope**: Only implements the operations we actually need
- **Learning Experience**: Provides hands-on experience with OT concepts
- **Compatibility**: Maintains compatibility with existing Avers-based systems

## Related Projects

- **Previous Client**: [all-o-stasis-avers][all-o-stasis-avers] (now deprecated)
- **Current Client**: [all-o-stasis-oxy][all-o-stasis-oxy] (uses this backend)

## Resources

- [Original Avers Implementation][avers]
- [Previous Client Implementation][all-o-stasis-avers]
- [Current Client Implementation][all-o-stasis-oxy]

[avers]: https://github.com/wereHamster/avers
[all-o-stasis-avers]: https://github.com/iff/all-o-stasis-avers
[all-o-stasis-oxy]: https://github.com/iff/all-o-stasis-oxy
