# CWPL-1.0: Charlotte's Weak Copyleft Public License

The Mozilla Public License 2.0 with a network clause. Copyleft is per file, like MPL: changes
to the licensed files stay open, and the files can sit beside code under any other licence.
On top of that, someone who modifies the software and lets others use it over a network has to
offer those users the source.

The full text is in [LICENSE](LICENSE). It is MPL-2.0 changed under MPL's Section 10.3 (renamed,
license steward replaced) in two places:

- **Section 3.6, Use Over a Network.** If you modify covered software and make it, or a larger
  work containing it, available to anyone other than you over a network, you must offer every
  such user the source of the covered files, including your changes, at no charge and
  prominently (a link in the interface is enough). It reaches only the covered files, not the
  rest of a larger work, and it does not apply to unmodified copies.
- **Section 1.12, Secondary License.** Only AGPL-3.0 or later, which has its own network
  clause, instead of MPL's GPL-2.0, LGPL-2.1, AGPL-3.0 and later. So covered files cannot be
  relicensed under plain GPL to drop the network rule. Code under any other licence still
  combines with them as a larger work (Section 3.3).

There is no attribution clause, unlike CPAL.

## Using it

Put [LICENSE](LICENSE) at the root of the project. Optionally add this to each source file
(Exhibit A):

```
This Source Code Form is subject to the terms of Charlotte's Weak
Copyleft Public License, v. 1.0. If a copy of the CWPL was not
distributed with this file, You can obtain one from whoever
distributed this file to You.
```

In a `Cargo.toml` or `package.json`, the licence field is `LicenseRef-CWPL-1.0`, the SPDX form
for a licence that is not on SPDX's list.

## Things to know

- It is not OSI-approved and not on SPDX's list, so some companies block it the way they block
  any unknown licence.
- Covered files can be combined into AGPL-3.0 projects, but not into plain GPL or LGPL ones
  under those licences.
- The text has not been reviewed by a lawyer.
