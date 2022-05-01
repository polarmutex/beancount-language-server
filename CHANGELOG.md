# Changelog

### [1.0.3](https://github.com/polarmutex/beancount-language-server/compare/v1.0.2...v1.0.3) (2022-05-01)


### Bug Fixes

* update nix flake to build lsp ([00e97af](https://github.com/polarmutex/beancount-language-server/commit/00e97af413103a240fe6bdcbdad52bd8a4db170a))

### [1.0.2](https://github.com/polarmutex/beancount-language-server/compare/v1.0.1...v1.0.2) (2022-04-28)


### Bug Fixes

* cargo deny errors ([4ccef65](https://github.com/polarmutex/beancount-language-server/commit/4ccef655934b6a5df55c1a34e1d4a36f728c9814))
* cargo doc warnings ([7604072](https://github.com/polarmutex/beancount-language-server/commit/76040720849a0b1326fd19eef0cf884801828d35))
* clippy warnings ([8bf3cb8](https://github.com/polarmutex/beancount-language-server/commit/8bf3cb881ac0f92b59bf5c7655ab363d2ddb0dba))
* fixes [#143](https://github.com/polarmutex/beancount-language-server/issues/143) - add stdio option to keep options silimar to typescript ([32c3441](https://github.com/polarmutex/beancount-language-server/commit/32c34417056283e9d1ed6997942dfce169f45180))
* fixes [#53](https://github.com/polarmutex/beancount-language-server/issues/53) only log with specified as an option ([e755ce6](https://github.com/polarmutex/beancount-language-server/commit/e755ce6de820da8ed101778d78b5457a9f58ad0e))
* formatting errors ([1467afb](https://github.com/polarmutex/beancount-language-server/commit/1467afbe91df87ad33c88dfc18a713588965f68a))
* rust compiler warnings ([0c86d6c](https://github.com/polarmutex/beancount-language-server/commit/0c86d6c0d36d2fb9cfd463dca10ad428893b5d24))

### [1.0.1](https://github.com/polarmutex/beancount-language-server/compare/v1.0.0...v1.0.1) (2022-01-21)

### Bug Fixes

* activate document formatting by default ([0f82147](https://github.com/polarmutex/beancount-language-server/commit/0f821474e0216aaa1018c1fc451903b024089d12))

### 1.0.0 (2021-11-12)

### Bug Fixes

* completion node handling ([72c9c4c](https://github.com/polarmutex/beancount-language-server/commit/72c9c4ca8270b718a83db6391462cc2ae5add858))
* diagnostics not being cleared when going to no diagnostics ([82bd84c](https://github.com/polarmutex/beancount-language-server/commit/82bd84cfd0f0eb39796e13fa3129693c3f1d1b3e))
* do ci only on PR ([f1c00ce](https://github.com/polarmutex/beancount-language-server/commit/f1c00cec3bd761c9a1482c5063c58cd53d4e1e46))
* ext before testing ([31d62a3](https://github.com/polarmutex/beancount-language-server/commit/31d62a337986abed909d276adbb8f515053b74d4))
* github funding ([9b13a15](https://github.com/polarmutex/beancount-language-server/commit/9b13a151eaca21a3a6fe0e015cb37d01d4a5a957))
* invalid date error ([db31d61](https://github.com/polarmutex/beancount-language-server/commit/db31d61bf40fc8f5325dc4e57628805acf08afcf))
* Nil compare for diagnostics ([91330be](https://github.com/polarmutex/beancount-language-server/commit/91330be0d905e489445608acebc69124c5ff2c5c))
* some clippy warnings ([b814eaa](https://github.com/polarmutex/beancount-language-server/commit/b814eaa250d515ef54520b7b97e3b096393ded39))
* tree-sitter v0.20 fix ([9dcef83](https://github.com/polarmutex/beancount-language-server/commit/9dcef83274b60324a4ca986be6b812649ce150b1))
* txn_string completion ([81b5ded](https://github.com/polarmutex/beancount-language-server/commit/81b5ded4c98ca1a280d930d046f3ef15111da131))

### Features

* account completion ([3745563](https://github.com/polarmutex/beancount-language-server/commit/3745563924a1d41e8216bd8e4cb0ce6a54244f23))
* add ability to call bean-check ([a8d4609](https://github.com/polarmutex/beancount-language-server/commit/a8d46091fe429e420c198d92851da427b6c6edd7))
* add ability to change python path to lsp ([84041f2](https://github.com/polarmutex/beancount-language-server/commit/84041f2786e2a5072495ec382dfaa937218d68ac))
* add diagnostics from bean-check ([6f1f5de](https://github.com/polarmutex/beancount-language-server/commit/6f1f5dede8f30adee7aba90c793d54011cbf240c))
* add initial set of rust ci ([313afe0](https://github.com/polarmutex/beancount-language-server/commit/313afe0fab3593f196084f5231702f2423ed8faa))
* add on save ([9f014ac](https://github.com/polarmutex/beancount-language-server/commit/9f014ac802e496a474652c1494ae81aec6bf297e))
* add start of document formatting ([b585367](https://github.com/polarmutex/beancount-language-server/commit/b5853679295c92330eaee4ca30dd0e6a29d357a2))
* add start of formatting tests ([2ed4cc3](https://github.com/polarmutex/beancount-language-server/commit/2ed4cc3d41596c535dfa6c7e8f81408df29d33b5))
* addded initial basic completions ([52a8e55](https://github.com/polarmutex/beancount-language-server/commit/52a8e55a9d0753a03f44903a4de9e297708e3f6c))
* added bean-check diagnostics ([3ef3523](https://github.com/polarmutex/beancount-language-server/commit/3ef3523482e47756f13d9fc57f06831056ab6dd4))
* added Data completion ([c72a0cd](https://github.com/polarmutex/beancount-language-server/commit/c72a0cd48a0cce61722a5b43c56196547e4e92cb))
* added flag entries to diagnostics ([17fe261](https://github.com/polarmutex/beancount-language-server/commit/17fe26159cf7eb4a4fffc7eff2357a7cbe14d014))
* added warning for flagged entries ([c7bd60d](https://github.com/polarmutex/beancount-language-server/commit/c7bd60d757bf0332fdd731991fe922bbb2826271))
* basic doc formatting is good shape ([d6ca9e2](https://github.com/polarmutex/beancount-language-server/commit/d6ca9e25d1edc45e51a4bdb124d08a4257f48bd8))
* basic doc formatting test done ([d30b2b7](https://github.com/polarmutex/beancount-language-server/commit/d30b2b707a0ff567dc98e536c3bd273818e58b9f))
* completion framework ([a70617e](https://github.com/polarmutex/beancount-language-server/commit/a70617e0582b58e7a83dc35efc57fc60f40cdfea))
* completion of date ([47a3527](https://github.com/polarmutex/beancount-language-server/commit/47a352760070f20605b87ba688f5417df2ac819c))
* editing tree on save done ([b6e3d2e](https://github.com/polarmutex/beancount-language-server/commit/b6e3d2e93963c7fedd4ec461fbd67977be6bdce2))
* formatting ([179c798](https://github.com/polarmutex/beancount-language-server/commit/179c798c62fa820d20a39f3d5e164714851681d6))
* import recursion on load to populate forest ([e81193a](https://github.com/polarmutex/beancount-language-server/commit/e81193a4add543f2a82ee62255d2f301a8161e89))
* initial lsp tests, impl didOpen ([32b61e7](https://github.com/polarmutex/beancount-language-server/commit/32b61e7acea84a01e42ff916acafae63050e74b6))
* initial README ([0035be2](https://github.com/polarmutex/beancount-language-server/commit/0035be2fe15267baf9a02efe4e5d1c9b5cdd6c7c))
* initial vs code ext from release ([8ceed50](https://github.com/polarmutex/beancount-language-server/commit/8ceed50e1c16788a059dc7ef50c46086178a66b3))
* initialize tests ([a860983](https://github.com/polarmutex/beancount-language-server/commit/a86098316570e9524b9a03a035b4f6d70ea554a2))
* reorg, added TS parsing on launch ([7af1a88](https://github.com/polarmutex/beancount-language-server/commit/7af1a886010a8bd3308b7ae3df47f6bca237e5d3))
* restructure add lerna ([54b3d44](https://github.com/polarmutex/beancount-language-server/commit/54b3d44da223c4c87ae19b27176efa48fe3fce3d))
* successfully calling bean-check ([1681217](https://github.com/polarmutex/beancount-language-server/commit/1681217b749e6965209b4629365f8e9295ca0275))
* support diagnostics for flagged entries ([4a4b1f3](https://github.com/polarmutex/beancount-language-server/commit/4a4b1f379aa658f7559cdddfd04f6dad978bbe41))
* switch to injection ([58e8cca](https://github.com/polarmutex/beancount-language-server/commit/58e8ccaed5470f1a10f63459ce98c2b1799c9387))
* switching to injection ([8c30e74](https://github.com/polarmutex/beancount-language-server/commit/8c30e74c4dcbad86d1c65173fe2385b718d4e44e))
* tree-sitter parse on open files ([1f5d836](https://github.com/polarmutex/beancount-language-server/commit/1f5d836af3136a438043fdc5458ddf6fcab781b7))
* txn_string completion ([94d57fc](https://github.com/polarmutex/beancount-language-server/commit/94d57fc3e5d015ddacbb6528a18081f0633e9331))
* updated tree-sitter wasm to v2 ([d5765cb](https://github.com/polarmutex/beancount-language-server/commit/d5765cb88268ba450291a092926c88a48e4bbf73))
