# Changelog

## [0.8.0](https://github.com/caelurium/evanescent/compare/v0.7.0...v0.8.0) (2026-11-28)


### ⚠ BREAKING CHANGES

* The spec.apiPostgres field is retired. Migrate to an approved OCI chart or delete the old environment before upgrading.
* The spec.apiPostgres field is retired. Migrate to an approved OCI chart or delete the old environment before upgrading.

### Features

* retire the bundled Postgres stack ([fa00f99](https://github.com/caelurium/evanescent/commit/fa00f99e74d3b5d2ce3dee5d735f5603e258df93))
* retire the bundled Postgres stack ([498e10b](https://github.com/caelurium/evanescent/commit/498e10b8dc6b1273d54fa3f1334374efe1253d0a))

## [0.7.0](https://github.com/caelurium/evanescent/compare/v0.6.0...v0.7.0) (2026-11-28)


### Features

* enforce preview ingress policy and namespace quotas ([06502a3](https://github.com/caelurium/evanescent/commit/06502a328613dee2ae12a677a4053aeb18470d9a))

## [0.6.0](https://github.com/caelurium/evanescent/compare/v0.5.0...v0.6.0) (2026-11-28)


### Features

* deploy approved OCI Helm charts ([f4b8a34](https://github.com/caelurium/evanescent/commit/f4b8a34d537e0acdb2a1e8e583a71aa48a274da6))
* deploy approved OCI Helm charts ([e810734](https://github.com/caelurium/evanescent/commit/e810734cf19230cf2b0a6cf0f6f98c5b85c61ccf))


### Bug Fixes

* record the source of catalog releases ([e70ba8c](https://github.com/caelurium/evanescent/commit/e70ba8c5ed64df2fa5e86ba87249481245ecaaaf))

## [0.5.0](https://github.com/caelurium/evanescent/compare/v0.4.0...v0.5.0) (2026-11-27)


### Features

* publish versioned operator images to GHCR ([f0dfe09](https://github.com/caelurium/evanescent/commit/f0dfe0978412048a39b2052ae8078d91bd875873))

## [0.4.0](https://github.com/caelurium/evanescent/compare/v0.3.0...v0.4.0) (2026-11-27)


### Features

* expire environments after an optional lifetime ([bbd7bd9](https://github.com/caelurium/evanescent/commit/bbd7bd9f9fdad8b0aaf99c1ce458f0b52be0f037))
* expire environments after an optional lifetime ([0ba70ac](https://github.com/caelurium/evanescent/commit/0ba70acbb7b5cb91802cb24e6cbf470f3476e14d))

## [0.3.0](https://github.com/caelurium/evanescent/compare/v0.2.0...v0.3.0) (2026-11-27)


### Features

* bundle Helm in the operator image ([784a505](https://github.com/caelurium/evanescent/commit/784a505c5ea3df6e84a63a05451e1e7e2cbae1d2))
* bundle Helm in the operator image ([ba62b8f](https://github.com/caelurium/evanescent/commit/ba62b8f6bd88c86b0de3a6a5c04c36d7cdcd57a5))
* reconcile a bundled Helm stack ([9574f15](https://github.com/caelurium/evanescent/commit/9574f159294640c0e255c7dea9774d20974594c8))
* reconcile a bundled Helm stack ([0c8be65](https://github.com/caelurium/evanescent/commit/0c8be65d1d5f3bc1228bab081f9bd43f0e1ee3a8))

## [0.2.0](https://github.com/caelurium/evanescent/compare/v0.1.0...v0.2.0) (2026-11-27)


### Features

* define the EphemeralEnvironment API ([c33e33c](https://github.com/caelurium/evanescent/commit/c33e33c86467c6d64ec2e9d17eb8a40058c66b35))
* reconcile ephemeral environment namespaces ([5e651a2](https://github.com/caelurium/evanescent/commit/5e651a2c2a5487186a575fddd8007e7dd9eeac91))
* reconcile ephemeral environment namespaces ([b86f8c0](https://github.com/caelurium/evanescent/commit/b86f8c0fa9ff88b9b0a7b3e058eb6a89976c1eb2))

## 0.1.0 (2026-11-27)


### Features

* bootstrap the Rust crate and CI ([2a58de4](https://github.com/caelurium/evanescent/commit/2a58de4ba2a4c08a994350015cb20d5cdfd85282))
* bootstrap the Rust crate and CI ([b0acac0](https://github.com/caelurium/evanescent/commit/b0acac0c4d8fd2f4ac0e675b77ad6f32ffa3441c))
