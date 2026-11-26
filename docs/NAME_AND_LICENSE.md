# Name and licensing

Research snapshot from November 25, 2026, during phase 0.

**ShortCircuit** is the project name, and the owner approved using it for the public development repository. Release naming clearance remains pending. The initial search found URL shorteners with similar names and a US trademark record for software. Name rights and the likelihood of confusion depend on the territory, actual use, and the goods and services involved.

**MIT** is the project license selected by the owner in the initial GitHub commit. The licensing survey below records other options considered during planning. [MIT License](../LICENSE)

## Software, domains, and package names

The initial survey searched public GitHub repositories for `shortcircuit in:name` and for name and README results with `"url shortener"` or `"link management"`. It reviewed relevant README files and searched Docker Hub and GHCR for names, images, and public package paths. The results reflect the research date.

| Source | Observation | Relevance |
|---|---|---|
| GitHub · Short-Circuit | A URL shortener with link management and visit tracking. The two repositories link to the same demo and screenshot, so the survey treats them as one project lineage. [Himansyou](https://github.com/Himansyou/shortcircuit/blob/main/README.md) · [Mindflayer09](https://github.com/Mindflayer09/shortcircuit/blob/master/README.md) | Similar name used in the same product category. |
| GitHub · Short Circuit | A URL shortener with custom short codes and light and dark themes. [AdityAiiitl03](https://github.com/AdityAiiitl03/ShortCircuit/blob/main/README.md) | Similar name used in the same product category. |
| GitHub · ShortCircuit | A URL shortener with analytics uses this exact repository name. [NitinJuyal1610](https://github.com/NitinJuyal1610/ShortCircuit/blob/main/README.md) | Identical repository spelling in the same product category. |
| GitHub · Shortcircuit XT and other results | Shortcircuit XT is an audio sampler. `secondfry/shortcircuit` is an EVE Online route planner. The GitHub account `shortcircuit` is taken. [Surge Synth Team](https://surge-synth-team.org/shortcircuit-xt/) · [secondfry](https://github.com/secondfry/shortcircuit) · [GitHub account](https://github.com/shortcircuit) | The project repository path depends on the selected owner. |
| Web · shortcircuit.io and shortcircuit.chat | The initial survey found a technical blog at `shortcircuit.io` and an Apple-platform AI assistant at `shortcircuit.chat`. [shortCircuit](https://shortcircuit.io/) · [Short Circuit](https://shortcircuit.chat/) | Existing domains and software uses. |
| Docker Hub · GridSuite | GridSuite deploys `docker.io/gridsuite/shortcircuit-server` for electrical short-circuit calculations. General Docker Hub searches and public tag queries were inconclusive. [GridSuite deployment](https://github.com/gridsuite/deployment/blob/7102ad61a9367a520f13fb0ec992b11faf37e2fc/k8s/resources/study/shortcircuit-server-deployment.yaml) · [GridSuite documentation](https://www.gridsuite.org/documentation/architecture/MicroServices.html) · [Docker namespaces](https://docs.docker.com/docker-hub/repos/create/) | A similar image name exists in another namespace. Availability of `<owner>/shortcircuit` remains open. |
| GHCR · `ghcr.io/<owner>/shortcircuit` | Public code and package searches found no confirmed shortener image with the same name. The direct tag endpoint for `shortcircuit/shortcircuit` and that owner's package list were unavailable to the survey. [GitHub Container Registry](https://docs.github.com/en/packages/working-with-a-github-packages-registry/working-with-the-container-registry) | The approved owner determines the package path. The survey covered public packages. |

The repository results document public use in the same product category. Trademark clearance requires its own review. Keep the design name provisional and make domains and image references configurable so a later name change leaves public short URLs and operational data intact.

## Trademark registers

The initial review checked official search portals and indexed results for `ShortCircuit`, `Short Circuit`, and `Short-Circuit`. The review lacks reproducible register results for all variants and target jurisdictions. A follow-up search should include phonetic and visual variants.

| Register | Research findings |
|---|---|
| Germany · DPMAregister | The official basic search was reachable. A usable export for the three variants was unavailable. The DPMA also refers applicants to the EU and international registers for the corresponding rights. [DPMA search guidance](https://www.dpma.de/recherchieren/schutzrechte-recherchieren/marken-recherchieren) · [DPMAregister](https://register.dpma.de/DPMAregister/marke/basis) |
| EU · EUIPO and TMview | eSearch returned an error and TMview results were inaccessible through the research connection. EU availability remains open. [EUIPO eSearch](https://euipo.europa.eu/eSearch/) · [TMview](https://www.tmdn.org/tmview/) |
| International · WIPO | The Global Brand Database and Madrid Monitor are relevant portals. The dynamic search yielded no verifiable results extract. Follow-up also requires the registers of the intended markets. [WIPO Global Brand Database](https://www.wipo.int/en/web/global-brand-database) · [Madrid Monitor](https://www3.wipo.int/madrid/monitor/en/) |
| United States · USPTO | The official TTAB records identify King Show Games' **SHORT CIRCUIT**, serial **87589897**, registration **5740606**. The accessible register mirror describes game and gambling software. Full current status was unavailable through TSDR during the survey. [USPTO TTABVUE](https://ttabvue.uspto.gov/ttabvue/v?pnam=King+Show+Games%2C+Inc.++) · [USPTO TSDR](https://tsdr.uspto.gov/#caseNumber=87589897&caseSearchType=US_APPLICATION&caseType=DEFAULT&searchType=status) · [Justia mirror](https://trademarks.justia.com/875/89/short-87589897.html) |
| United States · Other records | Secondary mirrors list **99153105** for printed goods and **50014686** for music recordings in class 9. The second record covers specific goods within that class. Both records warrant official verification. [USPTO 99153105](https://tsdr.uspto.gov/#caseNumber=99153105&caseSearchType=US_APPLICATION&caseType=DEFAULT&searchType=status) · [Mirror for 99153105](https://www.trademarkelite.com/trademark/trademark-detail/99153105/SHORT-CIRCUIT) · [USPTO 50014686](https://tsdr.uspto.gov/#caseNumber=50014686&caseSearchType=US_APPLICATION&caseType=DEFAULT&searchType=status) · [Mirror for 50014686](https://trademarks.justia.com/500/14/short-50014686.html) |

Before public distribution, review similar marks and commercial uses in the intended markets. Include the wordmark, logo, tagline, active and pending rights, and the actual goods, services, and audiences involved. Use class 9 for downloadable software and class 42 for relevant software services as search starting points. [WIPO Nice Classification](https://www.wipo.int/en/web/classification-nice) · [USPTO search guidance](https://www.uspto.gov/trademarks/search/federal-trademark-searching) · [DPMA search guide](https://www.dpma.de/fileadmin/user_upload/Dokumente/Recherchieren/recherchemarken_dt.pdf)

## License options evaluated

| License | Distribution implications | Fit |
|---|---|---|
| MIT | This permissive license requires retaining the copyright and permission notice when distributing covered copies. [Open Source Initiative](https://opensource.org/license/mit) | Broad reuse with limited administrative overhead. |
| Apache-2.0 | This permissive license includes an express patent grant and a patent termination clause. Distribution requires the license text, applicable notices, identification of modified files, and any required NOTICE material. Section 6 addresses trademarks. [Apache Software Foundation](https://www.apache.org/licenses/LICENSE-2.0) | Permissive distribution with explicit patent terms. |
| GPL-3.0 | Copyleft applies when conveying covered derivative works. Distributed binaries and container images require a corresponding source delivery or offer that meets the license terms. [SPDX GPL-3.0-only](https://spdx.org/licenses/GPL-3.0-only.html) | A candidate when distributed forks should provide corresponding source. |
| AGPL-3.0 | Section 13 adds a corresponding-source offer for remote users who interact with a modified, network-enabled version. [SPDX AGPL-3.0-only](https://spdx.org/licenses/AGPL-3.0-only.html) | A candidate when users of modified hosted forks should be able to obtain corresponding source. A hosted distribution would need a way to present the corresponding-source offer. |

Verify license compatibility for runtime dependencies and assets. Review the exact Rust and frontend dependency versions, statically linked libraries, embedded assets, and base image before the first approved release. Record the results in CI alongside a software bill of materials and the applicable third-party notices. The licenses of bundled components and their transitive dependencies govern use and distribution of those components.

QR logo verification uses the existing `rqrr` 0.11.0 dependency at runtime with default features disabled. Its license expression is `(MIT OR Apache-2.0) AND ISC`. The container build includes its license files from the Cargo source distribution. [Upstream manifest](https://github.com/WanzenBug/rqrr/blob/master/Cargo.toml)

Document the source path, version or commit, license, copyright, modifications, and distribution of any material the project incorporates. The ShortCircuit interface, product copy, and branding have their own design and asset inventory. [Design system](DESIGN_SYSTEM.md)

## Optional GeoIP data

The planned local GeoIP lookup uses a database supplied by the operator. For MaxMind GeoLite, the reviewed EULA distinguishes internal use from disclosure, references CC BY-SA 4.0 for copyrighted portions, and specifies attribution and removal of obsolete databases within 30 days after an update. Termination requires immediate removal. The ShortCircuit image and retained backups exclude the database file. The operator can supply a current, authorized copy for restore and use their own MaxMind account and license key to obtain it. The lookup processes visitor IP addresses locally. [MaxMind GeoLite EULA, sections 3 and 6](https://www.maxmind.com/en/geolite/eula) · [MaxMind downloads and updates](https://dev.maxmind.com/geoip/geolite2-free-geolocation-data/)

## Open decisions before public distribution

- Decide whether to publish under ShortCircuit after reviewing the name, tagline, and logo against current register records and market uses.
- Confirm the approved GitHub owner and, if needed, a Docker Hub namespace. Verify the intended paths and visibility.
- Confirm the distribution obligations for the release image.
