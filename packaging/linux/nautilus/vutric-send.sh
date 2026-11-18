#!/bin/sh
# SPDX-FileCopyrightText: 2026 Wonmor and Vutric contributors
# SPDX-License-Identifier: Apache-2.0
#
# Nautilus script "Vutric bilan yuborish" (installed per user by the daemon into ~/.local/share/nautilus/scripts/
# under its localised name, ADR-0091). Nautilus passes the selected local files as arguments.
[ "$#" -gt 0 ] || exit 0
exec vutric-daemon send -- "$@"
