import sys
import os
import json
import time
import random
import copy
import datetime

if sys.platform == "win32":
    try:
        import windows_curses as curses
    except ImportError:
        print("On Windows, install windows-curses: pip install windows-curses")
        sys.exit(1)
else:
    import curses

__version__ = "1.0.0"
__author__  = "Basanta Bhandari"

FIELD_W = 10
FIELD_H = 20

BLOCK = "##"
GHOST = "::"
HINT  = "[]"

DROP_BASE    = 0.5
DROP_MIN     = 0.05
DROP_PER_LVL = 0.04
AI_INTERVAL  = 0.08

LINE_SCORES   = [0, 100, 300, 500, 800]
LEADERBOARD_N = 10

SCORES_FILE = os.path.join(os.path.expanduser("~"), ".local", "share", "glyph", "pytris", "scores.json")
os.makedirs(os.path.dirname(SCORES_FILE), exist_ok=True)

ITALIC = "\x1B[3m"
RESET  = "\x1B[0m"

PIECE_ROTS = [
    [
        [(0,1),(1,0),(1,1),(2,0)],
        [(0,0),(1,0),(1,1),(2,1)],
    ],
    [
        [(0,0),(1,0),(1,1),(2,1)],
        [(0,1),(1,0),(1,1),(2,0)],
    ],
    [
        [(0,0),(0,1),(1,0),(1,1)],
    ],
    [
        [(0,0),(1,0),(2,0),(3,0)],
        [(0,0),(0,1),(0,2),(0,3)],
    ],
    [
        [(0,0),(1,0),(2,0),(2,1)],
        [(0,0),(0,1),(0,2),(1,0)],
        [(0,0),(0,1),(1,1),(2,1)],
        [(0,2),(1,0),(1,1),(1,2)],
    ],
    [
        [(0,1),(1,1),(2,0),(2,1)],
        [(0,0),(1,0),(1,1),(1,2)],
        [(0,0),(0,1),(1,0),(2,0)],
        [(0,0),(0,1),(0,2),(1,2)],
    ],
    [
        [(0,1),(1,0),(1,1),(1,2)],
        [(0,0),(1,0),(1,1),(2,0)],
        [(0,0),(0,1),(0,2),(1,1)],
        [(0,1),(1,0),(1,1),(2,1)],
    ],
]

COLOR_S      = 1
COLOR_Z      = 2
COLOR_O      = 3
COLOR_I      = 4
COLOR_J      = 5
COLOR_L      = 6
COLOR_T      = 7
COLOR_GHOST  = 8
COLOR_BORDER = 9
COLOR_LABEL  = 10
COLOR_TITLE  = 11

PIECE_COLORS = [COLOR_S, COLOR_Z, COLOR_O, COLOR_I, COLOR_J, COLOR_L, COLOR_T]

BANNER = [
    r" .----------------.  .----------------.  .----------------.  .----------------.  .----------------.  .----------------. ",
    r"| .--------------. || .--------------. || .--------------. || .--------------. || .--------------. || .--------------. |",
    r"| |   ______     | || |  ____  ____  | || |  _________   | || |  _______     | || |     _____    | || |    _______   | |",
    r"| |  |_   __ \   | || | |_  _||_  _| | || | |  _   _  |  | || | |_   __ \    | || |    |_   _|   | || |   /  ___  |  | |",
    r"| |    | |__) |  | || |   \ \  / /   | || | |_/ | | \_|  | || |   | |__) |   | || |      | |     | || |  |  (__ \_|  | |",
    r"| |    |  ___/   | || |    \ \/ /    | || |     | |      | || |   |  __ /    | || |      | |     | || |   '.___`-.   | |",
    r"| |   _| |_      | || |    _|  |_    | || |    _| |_     | || |  _| |  \ \_  | || |     _| |_    | || |  |`\____) |  | |",
    r"| |  |_____|     | || |   |______|   | || |   |_____|    | || | |____| |___| | || |    |_____|   | || |  |_______.'  | |",
    r"| |              | || |              | || |              | || |              | || |              | || |              | |",
    r"| '--------------' || '--------------' || '--------------' || '--------------' || '--------------' || '--------------' |",
    r" '----------------'  '----------------'  '----------------'  '----------------'  '----------------'  '----------------' ",
]


def prompt_username():
    for line in BANNER:
        print(line)
    print()
    username = input(f"  {ITALIC}>>> Enter Username: {RESET}").strip()
    if not username:
        username = "anonymous"
    return username


def load_scores():
    if not os.path.exists(SCORES_FILE):
        return []
    try:
        with open(SCORES_FILE, "r") as f:
            return json.load(f)
    except (json.JSONDecodeError, OSError):
        return []


def save_score(username, score, lines, level):
    scores = load_scores()
    today  = datetime.date.today().isoformat()

    existing = next((e for e in scores if e["user"] == username), None)
    if existing is None:
        scores.append({
            "user":  username,
            "score": score,
            "lines": lines,
            "level": level,
            "date":  today,
        })
    elif score > existing["score"]:
        existing["score"] = score
        existing["lines"] = lines
        existing["level"] = level
        existing["date"]  = today

    scores.sort(key=lambda e: e["score"], reverse=True)

    try:
        with open(SCORES_FILE, "w") as f:
            json.dump(scores, f, indent=2)
    except OSError:
        pass


def top_scores():
    return load_scores()[:LEADERBOARD_N]


def piece_cells(piece, rotation, px, py):
    rot = rotation % len(PIECE_ROTS[piece])
    return [(py + dr, px + dc) for dr, dc in PIECE_ROTS[piece][rot]]


def drop_speed(level):
    return max(DROP_MIN, DROP_BASE - (level - 1) * DROP_PER_LVL)


class Tetris:
    def __init__(self, computer=False, hint=False):
        self.field        = [[0] * FIELD_W for _ in range(FIELD_H)]
        self.field_colors = [[0] * FIELD_W for _ in range(FIELD_H)]
        self.score      = 0
        self.lines      = 0
        self.pieces     = 0
        self.level      = 1
        self.game_over  = False
        self.paused     = False
        self.computer   = computer
        self.hint       = hint
        self.start_time = time.time()

        self.next_piece = random.randint(0, 6)
        self.next_color = random.choice(PIECE_COLORS)
        self.dest_x     = 0
        self.dest_r     = 0

        self.piece = 0
        self.color = 0
        self.px    = 0
        self.py    = 0
        self.pr    = 0
        self._spawn()

    def _spawn(self):
        self.piece = self.next_piece
        self.color = self.next_color
        self.pr    = 0

        cells   = PIECE_ROTS[self.piece][0]
        piece_w = max(c for _, c in cells)
        piece_h = max(r for r, _ in cells)
        self.px = FIELD_W // 2 - piece_w // 2
        self.py = -piece_h - 1

        self.next_piece  = random.randint(0, 6)
        self.next_color  = random.choice(PIECE_COLORS)
        self.pieces     += 1

        if self.computer or self.hint:
            self._compute_dest()

    def cells(self):
        return piece_cells(self.piece, self.pr, self.px, self.py)

    def _valid(self, px, py, pr, piece=None):
        if piece is None:
            piece = self.piece
        for r, c in piece_cells(piece, pr, px, py):
            if c < 0 or c >= FIELD_W:
                return False
            if r >= FIELD_H:
                return False
            if r >= 0 and self.field[r][c]:
                return False
        return True

    def _drop_to_floor(self, px, pr, piece=None, start_py=None):
        py = self.py if start_py is None else start_py
        while self._valid(px, py + 1, pr, piece):
            py += 1
        return py

    def ghost_cells(self):
        gy = self._drop_to_floor(self.px, self.pr)
        return piece_cells(self.piece, self.pr, self.px, gy)

    def move_left(self):
        if self._valid(self.px - 1, self.py, self.pr):
            self.px -= 1

    def move_right(self):
        if self._valid(self.px + 1, self.py, self.pr):
            self.px += 1

    def rotate(self):
        nr = (self.pr + 1) % len(PIECE_ROTS[self.piece])
        for kick in (0, -1, 1, -2, 2):
            if self._valid(self.px + kick, self.py, nr):
                self.px += kick
                self.pr  = nr
                return

    def hard_drop(self):
        while self._valid(self.px, self.py + 1, self.pr):
            self.py    += 1
            self.score += 2
        self._lock()

    def soft_drop(self):
        if self._valid(self.px, self.py + 1, self.pr):
            self.py    += 1
            self.score += 1
        else:
            self._lock()

    def gravity(self):
        if self._valid(self.px, self.py + 1, self.pr):
            self.py += 1
        else:
            self._lock()

    def _lock(self):
        locked = self.cells()
        if any(r < 0 for r, c in locked):
            self.game_over = True
            return
        for r, c in locked:
            self.field[r][c]        = 1
            self.field_colors[r][c] = self.color
        self._clear_lines()
        self._spawn()

    def _clear_lines(self):
        full = [r for r in range(FIELD_H) if all(self.field[r])]
        for r in full:
            del self.field[r]
            del self.field_colors[r]
            self.field.insert(0, [0] * FIELD_W)
            self.field_colors.insert(0, [0] * FIELD_W)
        n = len(full)
        if n:
            self.score += LINE_SCORES[min(n, 4)] * self.level
            self.lines += n
            self.level  = self.lines // 10 + 1

    def _evaluate(self, px, pr, piece=None):
        if piece is None:
            piece = self.piece
        py = self._drop_to_floor(px, pr, piece, start_py=0)
        if not self._valid(px, py, pr, piece):
            return None

        field = copy.deepcopy(self.field)
        for r, c in piece_cells(piece, pr, px, py):
            if 0 <= r < FIELD_H and 0 <= c < FIELD_W:
                field[r][c] = 1

        complete = sum(1 for row in field if all(row))

        gaps = 0
        for c in range(FIELD_W):
            blocked = False
            for r in range(FIELD_H):
                if field[r][c]:
                    blocked = True
                elif blocked:
                    gaps += 1

        heights = []
        for c in range(FIELD_W):
            for r in range(FIELD_H):
                if field[r][c]:
                    heights.append(FIELD_H - r)
                    break
            else:
                heights.append(0)

        agg_height = sum(heights)
        bumpiness  = sum(abs(heights[c] - heights[c+1]) for c in range(FIELD_W - 1))

        return (complete * 3) - (gaps * 4) - agg_height - (bumpiness * 2)

    def _compute_dest(self):
        best_score = None
        best_x     = self.px
        best_r     = self.pr

        for r in range(len(PIECE_ROTS[self.piece])):
            for x in range(-2, FIELD_W + 2):
                score = self._evaluate(x, r)
                if score is None:
                    continue
                if best_score is None or score > best_score:
                    best_score = score
                    best_x     = x
                    best_r     = r

        self.dest_x = best_x
        self.dest_r = best_r

    def ai_step(self):
        target_r  = self.dest_r % len(PIECE_ROTS[self.piece])
        current_r = self.pr % len(PIECE_ROTS[self.piece])

        if current_r != target_r:
            self.rotate()
        elif self.px < self.dest_x:
            self.move_right()
        elif self.px > self.dest_x:
            self.move_left()
        else:
            self.gravity()

    def elapsed(self):
        return int(time.time() - self.start_time)


def init_colors():
    curses.start_color()
    curses.use_default_colors()
    curses.init_pair(COLOR_S,      curses.COLOR_RED,     curses.COLOR_RED)
    curses.init_pair(COLOR_Z,      curses.COLOR_GREEN,   curses.COLOR_GREEN)
    curses.init_pair(COLOR_O,      curses.COLOR_YELLOW,  curses.COLOR_YELLOW)
    curses.init_pair(COLOR_I,      curses.COLOR_CYAN,    curses.COLOR_CYAN)
    curses.init_pair(COLOR_J,      curses.COLOR_BLUE,    curses.COLOR_BLUE)
    curses.init_pair(COLOR_L,      curses.COLOR_MAGENTA, curses.COLOR_MAGENTA)
    curses.init_pair(COLOR_T,      curses.COLOR_WHITE,   curses.COLOR_WHITE)
    curses.init_pair(COLOR_GHOST,  curses.COLOR_WHITE,   -1)
    curses.init_pair(COLOR_BORDER, curses.COLOR_WHITE,   -1)
    curses.init_pair(COLOR_LABEL,  curses.COLOR_CYAN,    -1)
    curses.init_pair(COLOR_TITLE,  curses.COLOR_YELLOW,  -1)


def safe_addstr(win, y, x, text, attr=0):
    try:
        win.addstr(y, x, text, attr)
    except curses.error:
        pass


def draw_sidebar_row(stdscr, y, x, label, value):
    safe_addstr(stdscr, y,     x, label, curses.color_pair(COLOR_LABEL) | curses.A_BOLD)
    safe_addstr(stdscr, y + 1, x, str(value).rjust(10), curses.color_pair(COLOR_BORDER))


def render(stdscr, game, username):
    stdscr.erase()
    h, w = stdscr.getmaxyx()

    field_w = FIELD_W * 2 + 2
    ox      = max(0, (w - field_w - 20) // 2)
    oy      = max(0, (h - FIELD_H - 2) // 2)
    sx      = ox + field_w + 2

    title = f"-- PYTRIS  [{username}] --"
    safe_addstr(stdscr, max(0, oy - 2), ox + (field_w - len(title)) // 2, title,
                curses.color_pair(COLOR_TITLE) | curses.A_BOLD)

    border_attr = curses.color_pair(COLOR_BORDER)
    safe_addstr(stdscr, oy,               ox, "+" + "--" * FIELD_W + "+", border_attr)
    safe_addstr(stdscr, oy + FIELD_H + 1, ox, "+" + "--" * FIELD_W + "+", border_attr)
    for r in range(FIELD_H):
        safe_addstr(stdscr, oy + 1 + r, ox,                    "|", border_attr)
        safe_addstr(stdscr, oy + 1 + r, ox + 1 + FIELD_W * 2, "|", border_attr)

    for r in range(FIELD_H):
        for c in range(FIELD_W):
            if game.field[r][c]:
                safe_addstr(stdscr, oy + 1 + r, ox + 1 + c * 2, BLOCK,
                            curses.color_pair(game.field_colors[r][c]) | curses.A_BOLD)

    active = set(game.cells())
    for r, c in game.ghost_cells():
        if (r, c) not in active and 0 <= r < FIELD_H and 0 <= c < FIELD_W and not game.field[r][c]:
            safe_addstr(stdscr, oy + 1 + r, ox + 1 + c * 2,
                        GHOST, curses.color_pair(COLOR_GHOST) | curses.A_DIM)

    for r, c in game.cells():
        if 0 <= r < FIELD_H and 0 <= c < FIELD_W:
            safe_addstr(stdscr, oy + 1 + r, ox + 1 + c * 2,
                        BLOCK, curses.color_pair(game.color) | curses.A_BOLD)

    if game.hint and not game.computer:
        hint_y = game._drop_to_floor(game.dest_x, game.dest_r)
        for r, c in piece_cells(game.piece, game.dest_r, game.dest_x, hint_y):
            if 0 <= r < FIELD_H and 0 <= c < FIELD_W and not game.field[r][c]:
                safe_addstr(stdscr, oy + 1 + r, ox + 1 + c * 2,
                            HINT, curses.color_pair(COLOR_LABEL) | curses.A_DIM)

    draw_sidebar_row(stdscr, oy + 0,  sx, "SCORE",  game.score)
    draw_sidebar_row(stdscr, oy + 3,  sx, "LINES",  game.lines)
    draw_sidebar_row(stdscr, oy + 6,  sx, "LEVEL",  game.level)
    draw_sidebar_row(stdscr, oy + 9,  sx, "PIECES", game.pieces)
    draw_sidebar_row(stdscr, oy + 12, sx, "TIME",   game.elapsed())

    safe_addstr(stdscr, oy + 15, sx, "NEXT", curses.color_pair(COLOR_LABEL) | curses.A_BOLD)
    for dr, dc in PIECE_ROTS[game.next_piece][0]:
        safe_addstr(stdscr, oy + 17 + dr, sx + dc * 2,
                    BLOCK, curses.color_pair(game.next_color) | curses.A_BOLD)

    dim = curses.color_pair(COLOR_BORDER) | curses.A_DIM
    safe_addstr(stdscr, oy + FIELD_H - 4, sx, "arrows: move/rot", dim)
    safe_addstr(stdscr, oy + FIELD_H - 3, sx, "down:   soft drop", dim)
    safe_addstr(stdscr, oy + FIELD_H - 2, sx, "space:  hard drop", dim)
    safe_addstr(stdscr, oy + FIELD_H - 1, sx, "p:      pause",     dim)
    safe_addstr(stdscr, oy + FIELD_H,     sx, "esc:    menu",      dim)
    safe_addstr(stdscr, oy + FIELD_H + 1, sx, "q:      quit",      dim)

    if game.computer:
        safe_addstr(stdscr, oy + FIELD_H + 2, sx, "[AI MODE]",
                    curses.color_pair(COLOR_TITLE) | curses.A_BOLD)
    elif game.hint:
        safe_addstr(stdscr, oy + FIELD_H + 2, sx, "[HINT MODE]",
                    curses.color_pair(COLOR_TITLE) | curses.A_BOLD)

    stdscr.refresh()


def draw_menu(stdscr):
    stdscr.erase()
    h, w = stdscr.getmaxyx()

    menu_items = [
        ("1.  Normal Game",         curses.color_pair(COLOR_BORDER)),
        ("2.  Hint Mode",           curses.color_pair(COLOR_BORDER)),
        ("3.  Computer Mode",       curses.color_pair(COLOR_BORDER)),
        ("4.  Leaderboard",         curses.color_pair(COLOR_BORDER)),
        ("",                        0),
        ("Q.  Quit",                curses.color_pair(COLOR_BORDER) | curses.A_DIM),
        ("",                        0),
        ("by Basanta Bhandari",     curses.color_pair(COLOR_GHOST)  | curses.A_DIM),
        ("inspired by M. Fogleman", curses.color_pair(COLOR_GHOST)  | curses.A_DIM),
    ]

    total_h = len(BANNER) + 1 + len(menu_items)
    start_y = max(0, (h - total_h) // 2)

    for i, line in enumerate(BANNER):
        x = max(0, (w - len(line)) // 2)
        safe_addstr(stdscr, start_y + i, x, line,
                    curses.color_pair(COLOR_TITLE) | curses.A_BOLD)

    for i, (text, attr) in enumerate(menu_items):
        x = max(0, w // 2 - len(text) // 2)
        safe_addstr(stdscr, start_y + len(BANNER) + 1 + i, x, text, attr)

    stdscr.refresh()


def draw_leaderboard(stdscr):
    stdscr.erase()
    h, w = stdscr.getmaxyx()

    scores  = top_scores()
    header  = "LEADERBOARD  --  TOP 10"
    col_hdr = f"  {'#':<4}  {'PLAYER':<16}  {'SCORE':>8}  {'LINES':>6}  {'LEVEL':>5}  {'DATE'}"

    rows = [header, "", col_hdr, "-" * len(col_hdr)]
    for i, entry in enumerate(scores, 1):
        rows.append(
            f"  {i:<4}  {entry['user']:<16}  {entry['score']:>8}  "
            f"{entry['lines']:>6}  {entry['level']:>5}  {entry['date']}"
        )
    if not scores:
        rows.append("  no scores yet -- play a game!")

    rows += ["", "press any key to return"]

    start_y = max(0, h // 2 - len(rows) // 2)
    for i, text in enumerate(rows):
        x    = max(0, w // 2 - len(text) // 2)
        attr = curses.color_pair(COLOR_TITLE) | curses.A_BOLD if i == 0 else \
               curses.color_pair(COLOR_LABEL) | curses.A_BOLD if i == 2 else \
               curses.color_pair(COLOR_BORDER) | curses.A_DIM if text.startswith("-") else \
               curses.color_pair(COLOR_BORDER)
        safe_addstr(stdscr, start_y + i, x, text, attr)

    stdscr.refresh()
    stdscr.nodelay(False)
    stdscr.getch()


def _rank_of(username, current_score):
    scores = load_scores()
    ranked = sorted(scores, key=lambda e: e["score"], reverse=True)
    for i, entry in enumerate(ranked, 1):
        if entry["user"] == username:
            return i
    return None


def draw_game_over(stdscr, game, username, countdown=None):
    stdscr.erase()
    h, w = stdscr.getmaxyx()

    scores  = top_scores()
    pb      = next((e for e in scores if e["user"] == username), None)
    is_pb   = pb is None or game.score >= pb["score"]
    pb_line = "new personal best!" if is_pb else f"your best: {pb['score']}"

    rank     = _rank_of(username, game.score)
    rank_str = f"leaderboard rank: #{rank}" if rank else "not on leaderboard yet"

    cd_str   = f"returning to menu in {countdown}s..." if countdown is not None else "R  retry"

    rows = [
        ("GAME OVER",                      curses.color_pair(COLOR_TITLE)  | curses.A_BOLD),
        ("",                               0),
        (f"Score:   {game.score:>9}",      curses.color_pair(COLOR_BORDER)),
        (f"Lines:   {game.lines:>9}",      curses.color_pair(COLOR_BORDER)),
        (f"Level:   {game.level:>9}",      curses.color_pair(COLOR_BORDER)),
        (f"Pieces:  {game.pieces:>9}",     curses.color_pair(COLOR_BORDER)),
        (f"Time:    {game.elapsed():>8}s", curses.color_pair(COLOR_BORDER)),
        ("",                               0),
        (pb_line,                          curses.color_pair(COLOR_LABEL)  | curses.A_BOLD),
        (rank_str,                         curses.color_pair(COLOR_LABEL)  | curses.A_DIM),
        ("",                               0),
        (cd_str,                           curses.color_pair(COLOR_LABEL)  | curses.A_BOLD),
        ("M  menu",                        curses.color_pair(COLOR_BORDER)),
        ("Q  quit",                        curses.color_pair(COLOR_BORDER) | curses.A_DIM),
    ]

    start_y = max(0, h // 2 - len(rows) // 2)
    for i, (text, attr) in enumerate(rows):
        x = max(0, w // 2 - len(text) // 2)
        safe_addstr(stdscr, start_y + i, x, text, attr)

    stdscr.refresh()


def handle_input(game, key, last_drop, now):
    if key == curses.KEY_LEFT:
        game.move_left()
    elif key == curses.KEY_RIGHT:
        game.move_right()
    elif key == curses.KEY_UP:
        game.rotate()
    elif key == curses.KEY_DOWN:
        game.soft_drop()
        last_drop = now
    elif key == ord(' '):
        game.hard_drop()
        last_drop = now
    return last_drop


def run_game(stdscr, username, computer=False, hint=False):
    game      = Tetris(computer=computer, hint=hint)
    last_drop = time.time()
    last_ai   = time.time()

    stdscr.nodelay(True)
    stdscr.keypad(True)
    curses.curs_set(0)

    while True:
        now = time.time()

        try:
            key = stdscr.getch()
        except curses.error:
            key = -1

        if key in (ord('q'), ord('Q')):
            save_score(username, game.score, game.lines, game.level)
            return 'quit'
        if key == 27:
            save_score(username, game.score, game.lines, game.level)
            return 'menu'
        if key in (ord('p'), ord('P')):
            game.paused = not game.paused

        if not game.paused and not game.game_over:
            if not computer:
                last_drop = handle_input(game, key, last_drop, now)

            if computer and now - last_ai >= AI_INTERVAL:
                game.ai_step()
                last_ai = now

            if now - last_drop >= drop_speed(game.level):
                if not computer:
                    game.gravity()
                last_drop = now

        render(stdscr, game, username)

        if game.paused:
            stdscr.erase()
            h, w = stdscr.getmaxyx()
            for text, attr in [
                ("PAUSED",              curses.color_pair(COLOR_TITLE)  | curses.A_BOLD),
                ("",                    0),
                ("P  resume",           curses.color_pair(COLOR_LABEL)  | curses.A_BOLD),
                ("ESC  menu",           curses.color_pair(COLOR_BORDER)),
                ("Q  quit",             curses.color_pair(COLOR_BORDER) | curses.A_DIM),
            ]:
                safe_addstr(stdscr, h // 2, max(0, w // 2 - len(text) // 2), text, attr)
                h += 1
            stdscr.refresh()

        if game.game_over:
            save_score(username, game.score, game.lines, game.level)
            auto_return = 5
            deadline    = time.time() + auto_return
            stdscr.nodelay(True)
            while True:
                remaining = max(0, int(deadline - time.time()))
                draw_game_over(stdscr, game, username, countdown=remaining)
                try:
                    k = stdscr.getch()
                except curses.error:
                    k = -1
                if k in (ord('r'), ord('R')):
                    return 'retry'
                if k in (ord('m'), ord('M')):
                    return 'menu'
                if k in (ord('q'), ord('Q')):
                    return 'quit'
                if remaining == 0:
                    return 'menu'
                time.sleep(0.25)

        time.sleep(0.016)


def main(stdscr):
    init_colors()
    mode     = 'menu'
    computer = False
    hint     = False

    while True:
        if mode == 'menu':
            draw_menu(stdscr)
            stdscr.nodelay(False)
            while True:
                k = stdscr.getch()
                if k == ord('1'):
                    computer, hint = False, False
                    mode = 'game'
                    break
                elif k == ord('2'):
                    computer, hint = False, True
                    mode = 'game'
                    break
                elif k == ord('3'):
                    computer, hint = True, False
                    mode = 'game'
                    break
                elif k == ord('4'):
                    draw_leaderboard(stdscr)
                elif k in (ord('q'), ord('Q')):
                    return

        if mode == 'game':
            result = run_game(stdscr, username, computer=computer, hint=hint)
            if result == 'quit':
                return
            mode = 'game' if result == 'retry' else 'menu'


def run():
    global username
    username = prompt_username()
    try:
        curses.wrapper(main)
    except KeyboardInterrupt:
        pass
    print(f"Thanks for playing Pytris, {username}!")


if __name__ == "__main__":
    run()
