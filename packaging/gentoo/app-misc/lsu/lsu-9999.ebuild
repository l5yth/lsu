# Copyright (c) 2026 l5yth
# SPDX-License-Identifier: Apache-2.0

EAPI=8

inherit cargo git-r3

DESCRIPTION="Terminal UI for systemd services and latest journal lines"
HOMEPAGE="https://github.com/l5yth/lsu"
EGIT_REPO_URI="https://github.com/l5yth/lsu.git"

LICENSE="Apache-2.0"
SLOT="0"
KEYWORDS=""
IUSE=""
PROPERTIES="live"

RDEPEND="
	sys-apps/systemd
"
BDEPEND="
	virtual/rust
"

src_unpack() {
	git-r3_src_unpack
	cargo_live_src_unpack
}

src_compile() {
	cargo_src_compile --bin lsu
}

src_install() {
	cargo_src_install --bin lsu
	einstalldocs
}
