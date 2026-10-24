# regards-ispdb

A copy of Thunderbird's database of mail providers (the ISPDB, from
[thunderbird/autoconfig](https://github.com/thunderbird/autoconfig)): each
provider's settings file, so Regards can find an address's mail servers
without asking anyone. `regards_core::autoconfig` reads them on the computer;
nothing is sent to Mozilla.

## Licence

The files in `data/` are Mozilla's and stay under the **Mozilla Public
License 2.0** (`LICENSE`), unchanged. This crate is MPL-2.0 for that reason.
The rest of Regards is Apache-2.0 OR MIT: the MPL applies file by file, so
code that reads these files is not affected. The app's licence list
(Support › Licences) names this copy.

## Which version

`source.json` gives the repository, commit and date. To refresh, pick a commit
**at least two weeks old** (the project's rule for anything it ships), then:

```sh
node crates/regards-ispdb/update.mjs <full commit sha>
```

The script replaces `data/`, `LICENSE`, `source.json` and `src/lib.rs`, and
touches nothing else. Review the diff, then run the `regards-core` tests.
