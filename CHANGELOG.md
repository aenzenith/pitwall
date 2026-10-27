# Changelog

## [1.18.1](https://github.com/aenzenith/pitwall/compare/v1.18.0...v1.18.1) (2026-10-27)


### Bug Fixes

* **terminal:** the last two panes stay when a terminal closes or leaves them ([70a8813](https://github.com/aenzenith/pitwall/commit/70a881350e72e830e8a47be6cc0389ba960c6c44))

## [1.18.0](https://github.com/aenzenith/pitwall/compare/v1.17.0...v1.18.0) (2026-10-27)


### Features

* **accounts:** switch between Claude accounts from the Fuel page ([9b79c86](https://github.com/aenzenith/pitwall/commit/9b79c8630e6662ebfd7af7e6d68fb85095bd9762))

## [1.17.0](https://github.com/aenzenith/pitwall/compare/v1.16.0...v1.17.0) (2026-10-25)


### Features

* **terminal:** on Windows a new tab opens the shell picked, or as administrator ([a713d69](https://github.com/aenzenith/pitwall/commit/a713d693c99a01444df07d36724ce721c974c18b))
* **terminal:** the panel lays its terminals out in panes ([8887aec](https://github.com/aenzenith/pitwall/commit/8887aec5bbce83e71b2fab7a4fbd15871ee18433))
* **window:** ⌘W closes a tab at a time, then its panel or Files window ([29a102f](https://github.com/aenzenith/pitwall/commit/29a102f6bb699f76398dc63b3b7505b5237aa64f))


### Bug Fixes

* **backup:** an import offers projects without a star ([d3e5a76](https://github.com/aenzenith/pitwall/commit/d3e5a76b1db1f41c6c1df22756e36a4bd1300636))
* **claude:** a past session is opened once, and again once it ended ([7ba8d56](https://github.com/aenzenith/pitwall/commit/7ba8d5619e2df9c2a972c56d1eb60f74d554e568))
* **claude:** opening a session never hides another's open question ([d1c8d42](https://github.com/aenzenith/pitwall/commit/d1c8d42da7548223a8b03dfff0c0a63ed857b85b))
* **files:** Open in editor opens the file shown, names keep their letters ([81597f1](https://github.com/aenzenith/pitwall/commit/81597f130106e00f1a47ec2d35e87aa58ae1575c))
* **fuel:** the 90 % alert comes once a window ([bc423da](https://github.com/aenzenith/pitwall/commit/bc423da4f5b32b0d18f182344f8cb8322f193347))
* **servers:** the address, a busy port and an error follow the server ([9c90c11](https://github.com/aenzenith/pitwall/commit/9c90c1130594bca504442a5f9f4b9720dbb1bd37))
* **terminal:** a Claude tab keeps its session's whole name ([285d575](https://github.com/aenzenith/pitwall/commit/285d5750be31c84c8e925d8e17e6e77f80c9cac1))
* **terminal:** a dragged tab hands the keyboard back, and panes stay right ([b162e67](https://github.com/aenzenith/pitwall/commit/b162e671d3e98c6fda09a850626c86dbe904e4ba))


### Performance

* **terminal:** output reaches the window in batches ([c759e79](https://github.com/aenzenith/pitwall/commit/c759e79535b2acd1037dd23e3adb34dfeebf4e46))
* **ui:** hidden output, animations and big files cost less ([01e56ec](https://github.com/aenzenith/pitwall/commit/01e56ec975a6a67febfd05b12d10ee6f3dd8c0f6))

## [1.16.0](https://github.com/aenzenith/pitwall/compare/v1.15.0...v1.16.0) (2026-10-22)


### Features

* **terminal:** a web address in a terminal opens in the browser on ⌘-click ([02b53e8](https://github.com/aenzenith/pitwall/commit/02b53e8ce37b2ad057551f4c3b55bdae3c6c0dad))

## [1.15.0](https://github.com/aenzenith/pitwall/compare/v1.14.0...v1.15.0) (2026-10-22)


### Features

* **terminal:** the panel's project picker lists the favourites and open projects, or every one ([6a2c563](https://github.com/aenzenith/pitwall/commit/6a2c563ff8b65897210e8b19696abf828c764a4f))


### Bug Fixes

* **terminal:** History lists a Claude session from its start, before its log is written ([2c3191b](https://github.com/aenzenith/pitwall/commit/2c3191b3f7882ccbe1247d9c066956aad57c89a6))

## [1.14.0](https://github.com/aenzenith/pitwall/compare/v1.13.0...v1.14.0) (2026-10-22)


### Features

* **popover:** the menu bar's window lists the favourites and running projects, or every one ([c147ea9](https://github.com/aenzenith/pitwall/commit/c147ea98b5b6e6eb19e6414f2b871a7e032c3df8))
* **projects:** a project stays listed without its star, and the list shows all or the favourites and open ones ([d2b254a](https://github.com/aenzenith/pitwall/commit/d2b254aad0736a48de8924b5db3c3cbe7f625ca6))
* **projects:** a working folder for a project whose app sits in a subfolder ([8db7371](https://github.com/aenzenith/pitwall/commit/8db737155f41033ba9e2aacb194f484c64305688))
* **projects:** Focus the terminal, every place's way to open a project ([68bb891](https://github.com/aenzenith/pitwall/commit/68bb89125c9546a5e2f7f75462248614b2df8eac))
* **projects:** what opening a project opens, picked per place in Settings › Projects ([9579ba3](https://github.com/aenzenith/pitwall/commit/9579ba3f069d10086d4b1b7e8f2712ef1fa313d9))
* **sessions:** terminals under the Sessions page's list, and a session no longer running opens again in one ([c94732e](https://github.com/aenzenith/pitwall/commit/c94732e0b73b8200106909cffcff4b4a64ef05c4))
* **switcher:** a folder of the projects folder opens as Settings › Projects says, added without a star ([588c837](https://github.com/aenzenith/pitwall/commit/588c837090a6d6ae70b5b641ea8ee9a9d3fe6082))
* **teams:** the projects in no team get their own entry, last under the sidebar's teams ([b0cff9d](https://github.com/aenzenith/pitwall/commit/b0cff9d74a193bf4cb936c0eb698ce1961d06543))
* **terminal:** one terminal panel under every page, its outputs among its tabs ([0377009](https://github.com/aenzenith/pitwall/commit/0377009fabe89bc803c090ed307cf7aae258dd13))
* **terminal:** past Claude sessions beside the terminal, picked up again in a tab ([fccdbbe](https://github.com/aenzenith/pitwall/commit/fccdbbe0abaae9fe07d4114a87dfca064fd6ef73))
* **terminal:** tabs that drag into order, and one + for a shell or Claude ([b9f2332](https://github.com/aenzenith/pitwall/commit/b9f2332c0eb36eefa7c80dee95da18e99c515386))
* **today:** the day's Claude tokens and their share of the weekly limit ([30286c7](https://github.com/aenzenith/pitwall/commit/30286c754f9721e995f071f2cd2d2d32a3180ad4))


### Bug Fixes

* **files:** the Markdown preview's styles stay in the preview ([d2dc4fc](https://github.com/aenzenith/pitwall/commit/d2dc4fc0284106ca1792c9caf6cfc46c3984789f))
* **terminal:** picking a project on the Track page keeps the panel open, and the standings no longer stretch ([3ae824e](https://github.com/aenzenith/pitwall/commit/3ae824ea0ad03a757bdb0778d36a0896f75c90fb))

## [1.13.0](https://github.com/aenzenith/pitwall/compare/v1.12.3...v1.13.0) (2026-10-21)


### Features

* **details:** the dev server shows its address, and its buttons step back ([a45dae0](https://github.com/aenzenith/pitwall/commit/a45dae04996a45976c72f9bc81da9d3fc4474540))
* **files:** a project's files, its changes and its history in a window of their own ([1e512fe](https://github.com/aenzenith/pitwall/commit/1e512fe1f03d72fd53936e46ba5d6a689abebb6f))
* **teams:** a team's projects hang on one line, not under a head row ([386c70e](https://github.com/aenzenith/pitwall/commit/386c70ebc33e54fc52cb52bb191ac52f377e0a6d))
* **track:** the Track card takes the details' address line, and the standings open as the list does ([21f90af](https://github.com/aenzenith/pitwall/commit/21f90af71e61565482dffdc44c88f93c847b6bfe))


### Bug Fixes

* **claude:** hook events piled up while Pitwall was closed no longer stall its start ([d60274b](https://github.com/aenzenith/pitwall/commit/d60274bcfef7000a9eea2ffdf8cde7d986c967d3))
* **commands:** a command marked ask first never comes back on its own ([f9f8f95](https://github.com/aenzenith/pitwall/commit/f9f8f95e1b7dfe66485034ffaaacfba6b93730e1))
* **commands:** build commands run and can be saved like any other ([433b494](https://github.com/aenzenith/pitwall/commit/433b4947cabec5da4798e6484e1256c9bde50965))
* **deps:** a migration status read never goes through Poetry or Pipenv ([ea3f146](https://github.com/aenzenith/pitwall/commit/ea3f14622ebee0a6077221479697132eae4df23f))
* **discover:** a package.json that isn't a file is passed over ([85b06b3](https://github.com/aenzenith/pitwall/commit/85b06b3a86be35fd6308ce00b52f9f15c465df69))
* **git:** Pitwall's git reads run no program a repository names, and give up in time ([bed8661](https://github.com/aenzenith/pitwall/commit/bed8661defc80da8ea867668e86320c7af0a7504))
* **links:** the link dialog changes only its own project's links ([4cfe9fc](https://github.com/aenzenith/pitwall/commit/4cfe9fc7f174230c67fb892182a3eb8b0f12c57e))
* **projects:** a .env or package.json that isn't a plain file no longer freezes Pitwall ([49340b0](https://github.com/aenzenith/pitwall/commit/49340b0f1f0a666b6abdbb729c4470caf62f3236))
* **projects:** a damaged project list is set aside, never written over ([f02a577](https://github.com/aenzenith/pitwall/commit/f02a5778a50c5289304b544236a53705d5d5947a))
* **servers:** a process group that ended is never signalled again ([d5e8b3a](https://github.com/aenzenith/pitwall/commit/d5e8b3a30bf833a4e44b08a5616ef94e9c99f19a))
* **servers:** a project's folder is listed off the core's lock ([3013c9e](https://github.com/aenzenith/pitwall/commit/3013c9ed1f6e0ed9ba27ee3e0565ba5f5fe09028))
* **terminal:** clicking the tab already shown puts the keyboard in its terminal ([bcd79bc](https://github.com/aenzenith/pitwall/commit/bcd79bc96d0c0066a986ebead4df8a2635b78681))
* **terminal:** quitting ends what a tab runs in the background ([fb26f15](https://github.com/aenzenith/pitwall/commit/fb26f15331a045873c8087d3b94dcaec714b4f2e))
* **windows:** no window leaves Pitwall's own pages ([2a8569b](https://github.com/aenzenith/pitwall/commit/2a8569b212ab1a5f846b8a1aa2994cd8e73b6fec))

## [1.12.3](https://github.com/aenzenith/pitwall/compare/v1.12.2...v1.12.3) (2026-10-19)


### Bug Fixes

* **notify:** a Claude notification's click no longer covers the session ([616612a](https://github.com/aenzenith/pitwall/commit/616612a6402925c9729866c88b5bde9e5901c221))

## [1.12.2](https://github.com/aenzenith/pitwall/compare/v1.12.1...v1.12.2) (2026-10-18)


### Bug Fixes

* **start:** Fable is one of the models a session starts with ([1215292](https://github.com/aenzenith/pitwall/commit/12152922111cd361da1b8230e5674cb2d8b83cb1))
* **terminal:** going to a Claude tab counts its finished turn as seen ([36d2d6c](https://github.com/aenzenith/pitwall/commit/36d2d6c5ccf6c4f0d9dac4bbe3ab4261db02385d))
* **terminal:** New Claude's choices open on a right-click ([98214fe](https://github.com/aenzenith/pitwall/commit/98214fe6fd57376cc7544b5f4fa8d0480c9cbe6e))

## [1.12.1](https://github.com/aenzenith/pitwall/compare/v1.12.0...v1.12.1) (2026-10-16)


### Bug Fixes

* **deps:** a status read never makes a Python environment again ([eb37384](https://github.com/aenzenith/pitwall/commit/eb37384c7c9d115b659a38f0eb06464cbcee8ced))
* **hooks:** a hook of the user's own stays, and a linked settings file stays linked ([38f04ee](https://github.com/aenzenith/pitwall/commit/38f04eee152889e24cff8ca01a5840a0a9f81e7d))
* **terminal:** a long paste no longer freezes the app ([4af58f7](https://github.com/aenzenith/pitwall/commit/4af58f73cd712bc4afe37e01e428c74865908ee6))

## [1.12.0](https://github.com/aenzenith/pitwall/compare/v1.11.0...v1.12.0) (2026-10-15)


### Features

* **commands:** commands that ask first wait for you ([3b45c75](https://github.com/aenzenith/pitwall/commit/3b45c75743e5e22d52636fc4b67ec526f4a188aa))


### Bug Fixes

* **board:** cards and backups keep what they should ([ff508ab](https://github.com/aenzenith/pitwall/commit/ff508ab381cdbc0d646f84f9bb41f423bb1c22ac))
* **claude:** a turn stopped with Esc ends at once, quietly ([fb3b04e](https://github.com/aenzenith/pitwall/commit/fb3b04ee90721ffe7e1d3000dea90e71edfa67a9))
* **claude:** a working session shows at once, and a nested project's turn is told once ([5df5367](https://github.com/aenzenith/pitwall/commit/5df5367496e03e83c9b4d4a16a85898f3a905a33))
* **cli:** the command line and links, steadier ([9f06a82](https://github.com/aenzenith/pitwall/commit/9f06a828dd54dac207756411cfc07300d934305f))
* **commands:** a production build is refused however it is started ([44788e7](https://github.com/aenzenith/pitwall/commit/44788e7632e0a96a781ef1617a578dea4b962576))
* **fuel:** Claude Haiku 5.5 is priced ([9771d92](https://github.com/aenzenith/pitwall/commit/9771d928a75cb41a26d7d40e5a847f21a9468a72))
* **phone:** a refused message never shows part of the token ([202c275](https://github.com/aenzenith/pitwall/commit/202c275dc0fb2aca9038d02fee9f4bf20124afe5))
* **servers:** a server starts on its own port, and a missing folder says so ([83ea40f](https://github.com/aenzenith/pitwall/commit/83ea40f083ff32a8fc9f26d644ec2ab8ece947e7))
* **servers:** an error line follows the app's language ([a25aa46](https://github.com/aenzenith/pitwall/commit/a25aa46c79dec5baeb0447c0b1f560f594674cc9))
* **servers:** one server per project, wherever it is started ([55ce8d2](https://github.com/aenzenith/pitwall/commit/55ce8d278b9ccae0dc00b79d8bbc35b0e06e8bfb))
* **ui:** buttons stay in reach in a narrow window and with many tabs ([206b3fd](https://github.com/aenzenith/pitwall/commit/206b3fdafedd5fd93237ed27766d0f5fcf2a0e6f))

## [1.11.0](https://github.com/aenzenith/pitwall/compare/v1.10.0...v1.11.0) (2026-10-14)


### Features

* **capture:** a board card written in the quick switcher ([909bb10](https://github.com/aenzenith/pitwall/commit/909bb109c32c76970a557a416c33b83197147839))
* **cli:** the pitwall command and pitwall:// links ([0b4c966](https://github.com/aenzenith/pitwall/commit/0b4c966e68d96359ec99a5ac82df77c0f8289c89))
* **flag:** the chequered flag, what is still open before you leave ([ec7a757](https://github.com/aenzenith/pitwall/commit/ec7a75741edc64a528932f4dad5923c8dc757490))
* **projects:** a project's own three-letter code ([f9d9417](https://github.com/aenzenith/pitwall/commit/f9d94177de095ced7aaac4885a536df3212375fb))
* **setup:** the mini tower in step five, and a sixth step for alerts ([850b1b6](https://github.com/aenzenith/pitwall/commit/850b1b6bb5c877c042d8962e4bef885821d97307))
* **start:** Claude's model and mode, and cards given at the refill ([778c34f](https://github.com/aenzenith/pitwall/commit/778c34f8897a0ed188c64b06e794c8f169add00f))
* **tower:** the mini tower, a small window over every other ([fe4b956](https://github.com/aenzenith/pitwall/commit/fe4b9563118b9be4e5db6f3350d5b0b51e5de9e3))
* **track:** Track and Fuel in the light theme ([7ead96a](https://github.com/aenzenith/pitwall/commit/7ead96ad1b0ca21936664c9f7a68d1eb19918cf2))


### Bug Fixes

* **tray:** no crash at quit while the menu bar item redraws ([bb7c077](https://github.com/aenzenith/pitwall/commit/bb7c077c872d6d8bdf4f3e2f83da549aa0085347))

## [1.10.0](https://github.com/aenzenith/pitwall/compare/v1.9.0...v1.10.0) (2026-10-13)


### Features

* **backup:** export and import in Settings, and folders that moved ([fff83bc](https://github.com/aenzenith/pitwall/commit/fff83bc1c648c566b8f9440d7aa9d00729e8da7b))
* **notifications:** quiet mode, what you missed, a switch per event ([54fe93d](https://github.com/aenzenith/pitwall/commit/54fe93d21dff6452cec1e756fda850183979dbd4))
* **output:** search, an error filter, from an error to the editor ([d5433a1](https://github.com/aenzenith/pitwall/commit/d5433a11ba573b25cf6e7bae2ee03b9a292e5942))
* **phone:** the pit board on your phone, by ntfy, Telegram or a webhook ([26c7e5e](https://github.com/aenzenith/pitwall/commit/26c7e5efcecd5ecfbb710922632b22ee1292c2f5))
* **radio:** the team radio, Pitwall as an MCP server for Claude Code ([ae1bf8b](https://github.com/aenzenith/pitwall/commit/ae1bf8b8f8fd083510bfcb7ad2e9d4cf59effd57))
* **resume:** what was open at the last quit comes back at launch ([8b2f735](https://github.com/aenzenith/pitwall/commit/8b2f7352a2e62b119446f5368dc5aa89b9636332))
* **setup:** a first run in four steps, and new projects are offered ([dd7a770](https://github.com/aenzenith/pitwall/commit/dd7a770fc775ad33767cd2a32f56d966f5e06ba0))
* **teams:** projects in teams, ordered, started and stopped together ([6c5f57f](https://github.com/aenzenith/pitwall/commit/6c5f57f3e580bc544fb64d4ef1846c5b5a191bcd))
* **theme:** a light theme, chosen in Settings as System, Dark or Light ([aab69e1](https://github.com/aenzenith/pitwall/commit/aab69e17044ae2dd7641bd7eba80452b243272a2))
* **whats-new:** this version's notes, shown once after an update ([fce79cd](https://github.com/aenzenith/pitwall/commit/fce79cdbcca5be50a5f5237267bda3e9c1625e41))


### Bug Fixes

* **dialogs:** a dialog no longer opens with a button focused ([10bf8b8](https://github.com/aenzenith/pitwall/commit/10bf8b8597543295f9f76997f5f111e7349d6a4b))
* **servers:** a server restarted right after its start keeps its port ([0b1ec9e](https://github.com/aenzenith/pitwall/commit/0b1ec9e4204a08c4c631ec829e15c69ef8aef0ad))
* **tray:** the popover no longer opens on another screen ([e63768c](https://github.com/aenzenith/pitwall/commit/e63768c3748c7134a36984962dd31b18ae9fd0df))

## [1.9.0](https://github.com/aenzenith/pitwall/compare/v1.8.1...v1.9.0) (2026-10-12)


### Features

* **fuel:** Settings sets how often Claude's limits are asked for ([f89f0cc](https://github.com/aenzenith/pitwall/commit/f89f0ccf2827fcd592a8c7ea8e7738028c92f3d8))
* **popover:** Claude's fuel takes the foot of the popover ([6677371](https://github.com/aenzenith/pitwall/commit/667737108dca6dd0e98b983ef5ac5747cc90b5a9))
* **tray:** coloured counts and Claude's fuel beside the menu bar icon ([3733ff9](https://github.com/aenzenith/pitwall/commit/3733ff96c0e0c5931b3265e2c3dfb4dd0b5b4b80))

## [1.8.1](https://github.com/aenzenith/pitwall/compare/v1.8.0...v1.8.1) (2026-10-07)


### Bug Fixes

* **track:** a double click in the tower opens the editor ([130dfb5](https://github.com/aenzenith/pitwall/commit/130dfb5276217e566cd93df5aa14ef8bc988dcb2))

## [1.8.0](https://github.com/aenzenith/pitwall/compare/v1.7.1...v1.8.0) (2026-10-06)


### Features

* **sessions:** a session that just stopped opens again in a terminal ([3e08eb9](https://github.com/aenzenith/pitwall/commit/3e08eb9bbb72f72283e51bf1a89f9bef1094e36c))
* **terminal:** a Claude tab takes its session's name ([db9b622](https://github.com/aenzenith/pitwall/commit/db9b622c83a292c8a274c0f92d500fb4042d6943))


### Bug Fixes

* **deps:** a migrate is confirmed in a dialog ([654dccc](https://github.com/aenzenith/pitwall/commit/654dccc35d098d9d3c700518c8dc84082f942849))

## [1.7.1](https://github.com/aenzenith/pitwall/compare/v1.7.0...v1.7.1) (2026-10-05)


### Bug Fixes

* **claude:** a session stays at work through a long tool call ([c521d7b](https://github.com/aenzenith/pitwall/commit/c521d7b2005dea4831f89b458694806fb7be19c6))
* **claude:** a turn is announced once ([ee2eddb](https://github.com/aenzenith/pitwall/commit/ee2eddb30dcd70a892db5a1cdaad5e2f3c5fb7c8))
* **day:** a stash isn't listed among the day's commits ([771df75](https://github.com/aenzenith/pitwall/commit/771df756d652b3aa549fb2c8826c87f2d7c5cb09))
* **deps:** PHP's "&gt;8.1" and "&lt;=8.3" read as Composer reads them ([01102e7](https://github.com/aenzenith/pitwall/commit/01102e7ae781b69d26459cd49123d72580309213))
* **sessions:** "Working for" counts from the prompt ([50f22e1](https://github.com/aenzenith/pitwall/commit/50f22e12f33972041c14a4b13cf3d4ef2e2116ce))
* **switcher:** the highlight keeps to the match after an emoji ([da83e0f](https://github.com/aenzenith/pitwall/commit/da83e0f09e1c6352c779f3947ad481846ba4720c))
* **ui:** the sidebar's version is set smaller ([6dea488](https://github.com/aenzenith/pitwall/commit/6dea488190495cd405978d20da3c03bf52a05f94))

## [1.7.0](https://github.com/aenzenith/pitwall/compare/v1.6.0...v1.7.0) (2026-10-05)


### Features

* **sessions:** a project picker in place of "Listed projects only" ([dc261c5](https://github.com/aenzenith/pitwall/commit/dc261c5663e5753ad09585325cc2e6c8952b7c67))
* **update:** Pitwall updates itself and puts back what was open ([d52e274](https://github.com/aenzenith/pitwall/commit/d52e274149ec0b3f19d069545126ad8a67b6eda6))


### Bug Fixes

* **track:** a press on the stage folds the page's terminals ([2580fc3](https://github.com/aenzenith/pitwall/commit/2580fc3370d853425a117e8960fce58fc4c52993))
* **track:** an opening terminal panel leaves the circuit as it is ([93d460c](https://github.com/aenzenith/pitwall/commit/93d460c63505fb04b11f2cd6a352d798cd48b4f5))
* **track:** the page's terminals come up at 350 px ([5eca79e](https://github.com/aenzenith/pitwall/commit/5eca79ed71a7340dd77b0f49cd7c1c7ee7dcc7e9))

## [1.6.0](https://github.com/aenzenith/pitwall/compare/v1.5.0...v1.6.0) (2026-10-05)


### Features

* **track:** the page's terminals start collapsed ([89cc967](https://github.com/aenzenith/pitwall/commit/89cc9677ba0c6136384f54414a25c260b1cdfec6))


### Bug Fixes

* **terminal:** a collapsed panel leaves the keyboard alone ([ea803b2](https://github.com/aenzenith/pitwall/commit/ea803b2d7105fb50cad58bd1a76f28e207b46af4))

## [1.5.0](https://github.com/aenzenith/pitwall/compare/v1.4.1...v1.5.0) (2026-10-05)


### Features

* **claude:** a session comes up on the Track page when started there ([3ab01ce](https://github.com/aenzenith/pitwall/commit/3ab01cec6b5c33f3d21f333008579992d3382a13))
* **terminal:** Claude's state on a terminal's tab ([761078e](https://github.com/aenzenith/pitwall/commit/761078e8e862b8c707bd240078e3a72ef30bdaaf))
* **track:** the selected project's terminals along the page's bottom ([02c38d5](https://github.com/aenzenith/pitwall/commit/02c38d522d2c0892758227f427e51e3cff5bbedb))


### Bug Fixes

* **claude:** a session stays at work while its subagents run ([c4b59de](https://github.com/aenzenith/pitwall/commit/c4b59deec039a6ce5b4a21c747e8876bade196bf))

## [1.4.1](https://github.com/aenzenith/pitwall/compare/v1.4.0...v1.4.1) (2026-10-04)


### Bug Fixes

* **claude:** a session comes up at the card given last ([64be2da](https://github.com/aenzenith/pitwall/commit/64be2daf8808f5c6fc1f2833b748fe0a660ac143))
* **registry:** other participants' servers shown within a second ([7ebd1af](https://github.com/aenzenith/pitwall/commit/7ebd1afcff268b63779090f220274d882333f4d1))
* **sessions:** all time keeps counting a session's time ([0f3e4f4](https://github.com/aenzenith/pitwall/commit/0f3e4f42259be0487d85a2214c40b97711688d3a))
* **ui:** the days picker follows the clock past midnight ([0a7f6ff](https://github.com/aenzenith/pitwall/commit/0a7f6ff4a139d8ea2dcf66902b07075dfe37174e))

## [1.4.0](https://github.com/aenzenith/pitwall/compare/v1.3.0...v1.4.0) (2026-10-04)


### Features

* **board:** a mark at each column's head ([e94ff91](https://github.com/aenzenith/pitwall/commit/e94ff9125c98a3287b44d4bbbe298c96dc02ee7f))
* **board:** a note's image opens large ([38b3abf](https://github.com/aenzenith/pitwall/commit/38b3abf89ca6715bd4620d354443a7081f8ce070))
* **board:** the done cards of earlier days ([9f4fade](https://github.com/aenzenith/pitwall/commit/9f4fade02da5d0acd906fd6271d2f7861041fb17))
* **claude:** a session comes up where it was started ([36000a1](https://github.com/aenzenith/pitwall/commit/36000a129664d9cc05b03ed0f636ecd6d14e19a1))
* **sessions:** a session's terminal sits under its details ([255c291](https://github.com/aenzenith/pitwall/commit/255c291148b927e47ea9501d84446c9b7a0fa9d9))
* **sessions:** the sessions of earlier days ([e139598](https://github.com/aenzenith/pitwall/commit/e139598ee0b09ef1aa4b58b43b01eae2215141e3))
* **track:** each Claude session a light that drives to the pit wall ([03e56e9](https://github.com/aenzenith/pitwall/commit/03e56e9fd62a49294353e71451e5e146c19fd4a9))
* **ui:** the folder picker opens in the projects folder ([2f21681](https://github.com/aenzenith/pitwall/commit/2f21681005bfc59f05b6df1666bf6b5556274f15))


### Bug Fixes

* **board:** a selected card grows to its buttons instead of jumping ([e7f97ec](https://github.com/aenzenith/pitwall/commit/e7f97ecc99a7027053310cafb4371225559abc09))
* **fuel:** an ember's glow no longer ends at its cell's edge ([3b56fd8](https://github.com/aenzenith/pitwall/commit/3b56fd834e73f8861bba2b66f2ec3a31b5fbcdf3))
* **tray:** the popover fades in ([2b2000c](https://github.com/aenzenith/pitwall/commit/2b2000cc92f8821de27ee242f05a3156cbd409dc))
* **window:** the Claude filter is named "Needs attention" ([5611652](https://github.com/aenzenith/pitwall/commit/5611652f67ebbb7fb96d50a573d490b2bfeaac84))

## [1.3.0](https://github.com/aenzenith/pitwall/compare/v1.2.0...v1.3.0) (2026-10-04)


### Features

* **track:** a Track page, every project on a circuit ([a3170f8](https://github.com/aenzenith/pitwall/commit/a3170f8a3a6327022422449e26103190f7dcb2b9))


### Bug Fixes

* **board:** a card takes its session however late Claude lists it ([e0672c5](https://github.com/aenzenith/pitwall/commit/e0672c5c2b81cb6663f890756be2ff2458a88f06))
* **core:** the app leaves the Claude session it was started from ([8e9db7c](https://github.com/aenzenith/pitwall/commit/8e9db7c0cb26d82474fc375ddba95e207eb3d411))

## [1.2.0](https://github.com/aenzenith/pitwall/compare/v1.1.1...v1.2.0) (2026-10-03)


### Features

* **board:** a card in review follows its session back to Claude ([2805437](https://github.com/aenzenith/pitwall/commit/2805437e79d807c05c67cf808af7642fd4a8e2b9))
* **board:** images pasted into a card's note ([12d3f77](https://github.com/aenzenith/pitwall/commit/12d3f776d6f146a1593a78f2a6fd2ef03b78c621))
* **board:** the picker shows where Claude waits on you ([0b252a8](https://github.com/aenzenith/pitwall/commit/0b252a8e1bd4e157f1219caa2b85d79761ab6a3d))

## [1.1.1](https://github.com/aenzenith/pitwall/compare/v1.1.0...v1.1.1) (2026-10-03)


### Bug Fixes

* **shell:** commands run with the terminal's environment ([fe6c912](https://github.com/aenzenith/pitwall/commit/fe6c9124d07df94dabcf7c12c03cbf626d1d5236))

## [1.1.0](https://github.com/aenzenith/pitwall/compare/v1.0.0...v1.1.0) (2026-10-03)


### Features

* **a11y:** keyboard and VoiceOver through lists, tabs and timelines ([b63ce3a](https://github.com/aenzenith/pitwall/commit/b63ce3aca810150878dc97b515c9b7292e4c4276))
* **board:** a Board page, cards of work given to Claude ([012c713](https://github.com/aenzenith/pitwall/commit/012c7137f6267cdc000d1e8f9a546d976cbb0ef4))
* **browser:** "Open in browser" brings the project's tab forward ([b1e4cc7](https://github.com/aenzenith/pitwall/commit/b1e4cc77cb635d25aa3302525f14e988c4f369fe))
* **claude:** a session in the day view brought up where it runs ([90573f2](https://github.com/aenzenith/pitwall/commit/90573f225fdf215bef0f8accffbd6b4d6c6ff27b))
* **claude:** Claude's state at once, from its hooks and file events ([5f6b825](https://github.com/aenzenith/pitwall/commit/5f6b8255bc4a927d7d5d752fd4ab125e055546af))
* **day:** a timeline of the day, with Claude's sessions by name ([7d856ef](https://github.com/aenzenith/pitwall/commit/7d856ef230e3b589976f84c348c6e7f640b3f473))
* **day:** Claude's dot placed as in the other lists ([a9f022a](https://github.com/aenzenith/pitwall/commit/a9f022a457c211ff2fd9eb3280b016765f0e3e3e))
* **day:** the day's sessions listed without Claude's mark ([4012411](https://github.com/aenzenith/pitwall/commit/4012411a23b15ce47d58d010fe2927fc7200e9e2))
* **detail:** Claude's card right under the dev server ([ce5305e](https://github.com/aenzenith/pitwall/commit/ce5305e0464d0cb88756c1325d07ac382833a825))
* **detail:** Claude's card without a box, its state in the spark ([3906c22](https://github.com/aenzenith/pitwall/commit/3906c2229cfac5312d0a8ac1d8a165ad1e0b4be2))
* **detail:** no "open in its window" note under a stopped server ([f4f2713](https://github.com/aenzenith/pitwall/commit/f4f2713de7e36d9f76952896f8153f03529a9166))
* **fuel:** the cluster over a living ground, Claude's plan by its logo ([095ba9e](https://github.com/aenzenith/pitwall/commit/095ba9e11f99aaef304490ae7ada33c5a3d065bc))
* **fuel:** the Fuel page on the window's own ground ([01f552e](https://github.com/aenzenith/pitwall/commit/01f552ed86b28bf9ae952161df14abf28e1a41d6))
* **fuel:** the Fuel page, Claude's plan limits and today's tokens ([b74bbb6](https://github.com/aenzenith/pitwall/commit/b74bbb6990c223c8fe9dc16106ab510622905f3f))
* **garage:** a Garage page, each project's dependencies checked ([67124ca](https://github.com/aenzenith/pitwall/commit/67124ca4afd54f88296f5d874198faa9d9878b0d))
* **hooks:** Pitwall itself as Claude's hook on Windows and Linux ([b62d084](https://github.com/aenzenith/pitwall/commit/b62d0849b5aa9213435b924081ef4dbb36e57fcd))
* **links:** a project's other addresses, one click away ([859f94f](https://github.com/aenzenith/pitwall/commit/859f94fb1d45963ba6cdd99f72869c6b3d592faf))
* **platform:** notifications, sounds and the tray on Windows and Linux ([8146cb8](https://github.com/aenzenith/pitwall/commit/8146cb86233536895a15db844de4f64b294c5b91))
* **platform:** servers, shells and processes on Windows and Linux ([3ee61b3](https://github.com/aenzenith/pitwall/commit/3ee61b3aff7ff515bab3a9da5335c41b306214b6))
* **search:** case and accents don't count in a search ([8677c85](https://github.com/aenzenith/pitwall/commit/8677c858a03db11c265f686ec484a8c58f23e696))
* **sessions:** a Sessions page, every Claude Code session in one place ([610234e](https://github.com/aenzenith/pitwall/commit/610234eb34a5c21bf5c2490338a1e69d39fed84c))
* **sessions:** the session's terminal, or Claude's last message ([1a3a821](https://github.com/aenzenith/pitwall/commit/1a3a821c1decdfc844c741737f9ce455d547b123))
* **settings:** an icon on each settings tab ([4f0fbc9](https://github.com/aenzenith/pitwall/commit/4f0fbc9a3e19ce8b4d6eb337fb6def805dd39c02))
* **settings:** Pitwall for VS Code, and whether the editor has it ([c9654c4](https://github.com/aenzenith/pitwall/commit/c9654c48a5c49d6de0d54c4c6c98a989aafbacd5))
* **sounds:** Pitwall's own sounds, one per event ([fbc2a43](https://github.com/aenzenith/pitwall/commit/fbc2a43157369f85b59bc7253506e3bca1e0928f))
* **switcher:** `/` lists the window's pages ([9ae8475](https://github.com/aenzenith/pitwall/commit/9ae847584d8a06dfda926947db968e7b33d532c2))
* **switcher:** links searched and opened from the quick switcher ([03cff5f](https://github.com/aenzenith/pitwall/commit/03cff5ff57ff37004363875e92f4f4dcfc1ffee1))
* **switcher:** project commands run from the quick switcher ([efec8b5](https://github.com/aenzenith/pitwall/commit/efec8b512badfc7703f7c0e9362709e29c3db104))
* **ui:** Claude's filled logo everywhere, notifications included ([33e9337](https://github.com/aenzenith/pitwall/commit/33e93376ecca7966348f5d1fd344742019c0e223))
* **ui:** Ctrl for ⌘ and the system's title bar on Windows and Linux ([84ff4e3](https://github.com/aenzenith/pitwall/commit/84ff4e3cf518026b6b3dae84b46a4a36bb8196fd))
* **ui:** the hand cursor over whatever can be clicked ([bb913aa](https://github.com/aenzenith/pitwall/commit/bb913aaf9adb1e272df498718c17a4b788b0c342))
* **window:** dialogs closed by a click outside; sidebar entries with icons ([16b7e13](https://github.com/aenzenith/pitwall/commit/16b7e13224cc4a8654cc993f2ed7844747f9b972))


### Bug Fixes

* **core:** a live window's servers never reaped; starts and stops made safe ([434dfcc](https://github.com/aenzenith/pitwall/commit/434dfcc09ae6f4d47186f015b638b6a4f1da6eee))
* **i18n:** the Claude section's label read from the catalog ([0a888a5](https://github.com/aenzenith/pitwall/commit/0a888a58e1b1ef9e118a304e207fccf89daaa169))
* **notify:** macOS notifications shown again ([ebe34df](https://github.com/aenzenith/pitwall/commit/ebe34df23c9ea589e0dd93d7d438456480e2991b))
* **switcher:** opened apps no longer wait behind the switcher ([bef7fa6](https://github.com/aenzenith/pitwall/commit/bef7fa67f9c7910f86692bdf43ee458d78aa964f))
* **switcher:** the search never replaced by Pitwall's window ([22d12d6](https://github.com/aenzenith/pitwall/commit/22d12d6de32a68a82265be50839437e5c8476f3a))
* **tray:** the popover no longer brings Pitwall's window up ([3ca40dc](https://github.com/aenzenith/pitwall/commit/3ca40dcf664cb75ae37c4c0ad2695a424b92d3c6))
* **ui:** settings kept while typed, Enter on the switcher's own row ([d19e013](https://github.com/aenzenith/pitwall/commit/d19e013f05e499d1c10f935a8a76fbe754fc4294))
* **ui:** the sidebar marks the page shown with aria-current ([14b374c](https://github.com/aenzenith/pitwall/commit/14b374c9d618c4d52abdb7c29547b5f349d18cd9))


### Performance

* **window:** the main window and the switcher made on first use ([487fce9](https://github.com/aenzenith/pitwall/commit/487fce93282ac2afa6c8e39df9c4d11651c25f67))
