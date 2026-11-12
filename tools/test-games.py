#!/usr/bin/env python3
"""Regression checks for the imported games' persistent data."""
import importlib.util,os,tempfile,unittest
from pathlib import Path
from unittest.mock import patch
source=Path(__file__).resolve().parents[1]/'assets/games/terminal-spiel/save_system.py'
spec=importlib.util.spec_from_file_location('spiel_saves',source);module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
class GameSaves(unittest.TestCase):
 def test_current_player_state_survives_save_and_reload(self):
  with tempfile.TemporaryDirectory() as home,patch.dict(os.environ,{'HOME':home}):
   manager=module.SaveSlotManager()
   self.assertTrue(manager.save_game_to_slot(1,'Glyph',42,['potion'],['quest'],3,'evening','rain',7,reputation=12,energy=35))
   data=manager.load_game_from_slot(1)
   self.assertEqual((data['player_name'],data['gold'],data['reputation'],data['energy']),('Glyph',42,12,35))
   self.assertTrue(Path(manager.get_save_filename(1)).is_relative_to(home))
 def test_missing_or_invalid_slot_is_not_an_existing_save(self):
  with tempfile.TemporaryDirectory() as home,patch.dict(os.environ,{'HOME':home}):
   manager=module.SaveSlotManager();self.assertIsNone(manager.load_game_from_slot(1));self.assertIsNone(manager.load_game_from_slot(0))
if __name__=='__main__':unittest.main()
