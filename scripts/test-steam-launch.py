"""Exercise Steam argument/environment forwarding without launching a game or Proton."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).with_name("steam-launch.sh").resolve()
RECORDER = '''#!/usr/bin/env python3
import json, os, sys
from pathlib import Path
Path(os.environ["SA2_TEST_REPORT"]).write_text(json.dumps({
    "args": sys.argv[1:], "cwd": os.getcwd(), "app": os.environ["SteamAppId"]
}))
'''


class SteamLaunch(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="sa2 steam launch ")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.game = self.root / "Sonic Adventure 2"
        self.game.mkdir()
        self.report = self.root / "report.json"
        self.proton = self.root / "fake proton"
        self.proton.write_text(RECORDER)
        self.proton.chmod(0o755)
        self.environment = dict(os.environ, SA2_TEST_REPORT=str(self.report), SteamAppId="213610")

    def run_launch(self):
        return subprocess.run(
            ["bash", str(SCRIPT), str(self.proton), "waitforexitandrun",
             str(self.game / "Launcher.exe"), "--flag", "value with spaces"],
            env=self.environment, capture_output=True, text=True,
        )

    def test_updater_receives_original_steam_command_and_environment(self):
        updater = self.game / "mods/sa2-chao-online/sa2co-launcher"
        updater.parent.mkdir(parents=True)
        updater.write_text(RECORDER)  # wrapper also restores the extracted executable bit
        result = self.run_launch()
        self.assertEqual(result.returncode, 0, result.stderr)
        report = json.loads(self.report.read_text())
        self.assertEqual(report["args"], [str(self.proton), "waitforexitandrun",
                                         str(self.game / "sonic2app.exe"), "--flag", "value with spaces"])
        self.assertEqual(report["cwd"], str(self.game))
        self.assertEqual(report["app"], "213610")

    def test_an_old_installation_opens_manager_for_the_one_time_update(self):
        manager = self.game / "SAModManager.exe"
        manager.touch()
        result = self.run_launch()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(self.report.read_text())["args"],
                         ["waitforexitandrun", str(manager), "--flag", "value with spaces"])

    def test_missing_updater_and_manager_reports_an_error(self):
        result = self.run_launch()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("SA Mod Manager not found", result.stderr)
        self.assertFalse(self.report.exists())


if __name__ == "__main__":
    unittest.main()
