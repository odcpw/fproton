# Render `protonvpn config list` output offline, exactly as
# proton-vpn-cli 1.0.3 would print it (the CLI itself hangs on this machine).
# Logic transcribed from:
#   /usr/lib/python3.14/site-packages/proton/vpn/cli/commands/settings.py:285-331
#   .../commands/feature_setting_definitions.py:475-484 (ALL_FEATURES order)
#   .../commands/feature_setting_definitions.py:67-190  (to_str per click type)
#   .../core/controller.py:69 (available_on_free_tier default False)
from tabulate import tabulate
import sys

# (command, available_on_free_tier, paid-tier value string)
FEATURES = [
    ("netshield",               False, "malware-ads-trackers"),
    ("kill-switch",             True,  "standard"),
    ("port-forwarding",         False, "off"),
    ("custom-dns",              False, "on  [1.1.1.1, 8.8.8.8]"),
    ("vpn-accelerator",         False, "on"),
    ("moderate-nat",            False, "off"),
    ("ipv6",                    True,  "off"),
    ("anonymous-crash-reports", True,  "on"),
]
PROGRAM = "protonvpn"

def render(free_tier: bool) -> str:
    rows = [(c, "Upgrade to enable" if (not free_ok and free_tier) else v)
            for c, free_ok, v in FEATURES]
    table = tabulate(rows, headers=["Setting", "Value"], tablefmt="simple",
                     stralign="left", numalign="right")
    out = f"\nCurrent configuration\n{table}\n" + "\n"      # click.echo adds \n
    if free_tier:
        out += ("To upgrade to VPN Plus visit: https://account.protonvpn.com/pricing\n"
                "After upgrading online:\n"
                "    Sign out and sign in again to activate your new plan:\n"
                f"    {PROGRAM} signout && {PROGRAM} signin\n")
    else:
        s = f"{PROGRAM} config set"
        out += (f"Use '{s} <setting> <value>' to change settings.\n"
                f"Use '{s} <setting> --help' for available values.\n")
    return out

sys.stdout.write(render(free_tier=(len(sys.argv) > 1 and sys.argv[1] == "free")))
