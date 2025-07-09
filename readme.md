# Name to be decided

This is a simplified rewrite of [avers](https://github.com/wereHamster/avers) in Rust.

Motivations:

- migrating from RethinkDB to Firestore: simplifying cloud deployment
- having one API running that can host multiple different gyms
- learning how to write a backend in Rust

Shortcuts I took along the way:

- splitting the code in a simplified OT crate and the backend
- object types are currently hardcoded (boulders or accounts) so the OT crate can't be reused
- getting rid of some features the old backend implementation did not use (eg. no releases)

The previous app [all-o-stasis-avers](https://github.com/iff/all-o-stasis-avers) now uses this
backend: [all-o-stasis-oxy](https://github.com/iff/all-o-stasis-oxy).
