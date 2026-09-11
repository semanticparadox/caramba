#!/usr/bin/env python3
"""Публикация базы знаний docs/guides в Telegraph.

Зачем отдельный скрипт, а не редактор Telegraph: страницы живут в репозитории
(docs/guides/ru/*.md) как источник истины, их правят пул-реквестом, а Telegraph
получает результат. Ссылки на страницы лежат в настройках бота
(`guide_url_<id>`), поэтому уже опубликованные страницы редактируются НА МЕСТЕ
(`editPage` по сохранённому path), а новые создаются один раз (`createPage`) и
их path записывается обратно в docs/guides/pages.json.

Что умеет:
  * Markdown-подмножество -> узлы Telegraph (Node API): заголовки `##`/`###`
    (у Telegraph есть только h3/h4), абзацы, списки `-`/`1.`, цитаты `>`,
    линейки `---`, блоки кода ``` ```, картинки `![alt](url)` отдельной строкой,
    инлайн `**жирный**`, `*курсив*`, `` `код` ``, `[текст](url)`.
  * Перекрёстные ссылки между страницами: `[текст](guide:<id>)` подставляет URL
    страницы `<id>` из pages.json, так что markdown не знает про пути Telegraph.
  * `--dry-run`: печатает узлы (JSON) и сводку, в сеть не ходит.

Токен берётся ТОЛЬКО из переменной окружения TELEGRAPH_ACCESS_TOKEN, никогда не
печатается и не пишется в файлы: pages.json хранит только path и url, а любые
сообщения об ошибках содержат лишь ответ API.

Запуск (из корня репозитория):
  python3 scripts/telegraph-publish.py --dry-run
  TELEGRAPH_ACCESS_TOKEN=... python3 scripts/telegraph-publish.py
  TELEGRAPH_ACCESS_TOKEN=... python3 scripts/telegraph-publish.py --only faq
"""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Callable, Optional

API_BASE = "https://api.telegra.ph"
TOKEN_ENV = "TELEGRAPH_ACCESS_TOKEN"
# Лимит Telegraph на поле content (64 КБ). Проверяем до отправки, чтобы
# сломанная страница не уезжала наполовину.
CONTENT_LIMIT_BYTES = 64 * 1024
# Текст-заглушка, которым создаётся новая страница на первом проходе: path
# нужен раньше содержимого, потому что другие страницы ссылаются на него.
PLACEHOLDER_TEXT = "Страница обновляется, загляните через минуту."

Node = Any  # str | dict[str, Any]


# ---------------------------------------------------------------------------
# Инлайн-разметка
# ---------------------------------------------------------------------------

LinkResolver = Callable[[str], Optional[str]]


def _tag(tag: str, children: list[Node], attrs: Optional[dict[str, str]] = None) -> dict:
    node: dict[str, Any] = {"tag": tag}
    if attrs:
        node["attrs"] = attrs
    if children:
        node["children"] = children
    return node


def _push_text(out: list[Node], text: str) -> None:
    """Склеивает соседние строки: Telegraph принимает и раздельные, но JSON
    выходит компактнее, а тесты читаемее."""
    if not text:
        return
    if out and isinstance(out[-1], str):
        out[-1] += text
    else:
        out.append(text)


def _find_closing(text: str, start: int, marker: str) -> int:
    """Позиция закрывающего маркера с учётом экранирования обратным слэшем."""
    i = start
    while True:
        j = text.find(marker, i)
        if j < 0:
            return -1
        if j > 0 and text[j - 1] == "\\":
            i = j + 1
            continue
        return j


def _find_link_end(text: str, start: int) -> tuple[int, int]:
    """Для `[` в позиции start ищет `](` и закрывающую `)` с учётом вложенных
    скобок в тексте ссылки. Возвращает (позиция `](`, позиция `)`) или (-1, -1)."""
    depth = 0
    i = start
    while i < len(text):
        ch = text[i]
        if ch == "\\":
            i += 2
            continue
        if ch == "[":
            depth += 1
        elif ch == "]":
            depth -= 1
            if depth == 0:
                if text.startswith("](", i):
                    close = text.find(")", i + 2)
                    return (i, close) if close >= 0 else (-1, -1)
                return (-1, -1)
        i += 1
    return (-1, -1)


def _italic_boundary_ok(text: str, pos: int) -> bool:
    """`_` внутри слова (snake_case) курсивом не считается."""
    prev = text[pos - 1] if pos > 0 else " "
    return not (prev.isalnum())


def render_inline(text: str, resolve_link: Optional[LinkResolver] = None) -> list[Node]:
    """Инлайн-разметка -> список узлов Telegraph.

    Приоритет: `код` (внутри ничего не разбирается) > ссылки/картинки >
    **жирный** > *курсив* / _курсив_. Экранирование `\\*`, `\\_`, `\\``, `\\[`
    даёт символ как есть.
    """
    out: list[Node] = []
    i = 0
    n = len(text)
    while i < n:
        ch = text[i]

        if ch == "\\" and i + 1 < n and text[i + 1] in "\\*_`[]()!#>-":
            _push_text(out, text[i + 1])
            i += 2
            continue

        if ch == "`":
            j = _find_closing(text, i + 1, "`")
            if j > i:
                out.append(_tag("code", [text[i + 1 : j]]))
                i = j + 1
                continue

        if ch == "!" and i + 1 < n and text[i + 1] == "[":
            bracket, close = _find_link_end(text, i + 1)
            if bracket > 0:
                alt = text[i + 2 : bracket]
                src = text[bracket + 2 : close].strip()
                out.append(_tag("img", [], {"src": src, **({"alt": alt} if alt else {})}))
                i = close + 1
                continue

        if ch == "[":
            bracket, close = _find_link_end(text, i)
            if bracket > 0:
                label = text[i + 1 : bracket]
                href = text[bracket + 2 : close].strip()
                if resolve_link is not None:
                    resolved = resolve_link(href)
                    if resolved:
                        href = resolved
                out.append(_tag("a", render_inline(label, resolve_link), {"href": href}))
                i = close + 1
                continue

        if text.startswith("**", i):
            j = _find_closing(text, i + 2, "**")
            if j > i + 2:
                out.append(_tag("strong", render_inline(text[i + 2 : j], resolve_link)))
                i = j + 2
                continue

        if ch in "*_" and i + 1 < n and not text[i + 1].isspace():
            if ch == "*" or _italic_boundary_ok(text, i):
                j = _find_closing(text, i + 1, ch)
                # Для `_` закрывающий тоже обязан стоять на границе слова.
                while ch == "_" and j > 0 and j + 1 < n and text[j + 1].isalnum():
                    j = _find_closing(text, j + 1, ch)
                if j > i + 1 and not text[j - 1].isspace():
                    out.append(_tag("em", render_inline(text[i + 1 : j], resolve_link)))
                    i = j + 1
                    continue

        _push_text(out, ch)
        i += 1
    return out


# ---------------------------------------------------------------------------
# Блочная разметка
# ---------------------------------------------------------------------------

_RE_HEADING = re.compile(r"^(#{1,6})\s+(.*?)\s*#*\s*$")
_RE_UL = re.compile(r"^[-*+]\s+(.*)$")
_RE_OL = re.compile(r"^\d+[.)]\s+(.*)$")
_RE_HR = re.compile(r"^(?:-{3,}|\*{3,}|_{3,})\s*$")
_RE_IMAGE_LINE = re.compile(r"^!\[([^\]]*)\]\(([^)]+)\)\s*$")
_RE_FENCE = re.compile(r"^```")


@dataclass
class ParsedPage:
    title: str
    nodes: list[Node]


def _paragraph_nodes(lines: list[str], resolve_link: Optional[LinkResolver]) -> list[Node]:
    """Строки одного абзаца: два пробела или `\\` в конце строки дают <br>,
    иначе строки склеиваются пробелом."""
    children: list[Node] = []
    for idx, raw in enumerate(lines):
        line = raw.rstrip("\n")
        hard_break = line.endswith("  ") or line.endswith("\\")
        line = line.rstrip("\\").strip()
        for node in render_inline(line, resolve_link):
            if isinstance(node, str):
                _push_text(children, node)
            else:
                children.append(node)
        last = idx == len(lines) - 1
        if not last:
            if hard_break:
                children.append(_tag("br", []))
            else:
                _push_text(children, " ")
    return children


def parse_markdown(text: str, resolve_link: Optional[LinkResolver] = None) -> ParsedPage:
    """Markdown-подмножество -> заголовок страницы и узлы содержимого.

    Первый `# Заголовок` становится title страницы и в content не попадает.
    """
    title = ""
    nodes: list[Node] = []
    lines = text.splitlines()
    i = 0
    n = len(lines)

    while i < n:
        line = lines[i]
        stripped = line.strip()

        if not stripped:
            i += 1
            continue

        if _RE_FENCE.match(stripped):
            i += 1
            code: list[str] = []
            while i < n and not _RE_FENCE.match(lines[i].strip()):
                code.append(lines[i])
                i += 1
            i += 1  # закрывающий ```
            nodes.append(_tag("pre", ["\n".join(code)]))
            continue

        m = _RE_HEADING.match(stripped)
        if m:
            level = len(m.group(1))
            content = m.group(2)
            if level == 1 and not title:
                title = content
            else:
                # У Telegraph только h3 и h4: `##` -> h3, всё глубже -> h4.
                tag = "h3" if level <= 2 else "h4"
                nodes.append(_tag(tag, render_inline(content, resolve_link)))
            i += 1
            continue

        if _RE_HR.match(stripped):
            nodes.append(_tag("hr", []))
            i += 1
            continue

        m = _RE_IMAGE_LINE.match(stripped)
        if m:
            alt, src = m.group(1), m.group(2).strip()
            figure: list[Node] = [_tag("img", [], {"src": src})]
            if alt:
                figure.append(_tag("figcaption", [alt]))
            nodes.append(_tag("figure", figure))
            i += 1
            continue

        if stripped.startswith(">"):
            quote: list[str] = []
            while i < n and lines[i].strip().startswith(">"):
                quote.append(lines[i].strip()[1:].strip())
                i += 1
            nodes.append(_tag("blockquote", _paragraph_nodes(quote, resolve_link)))
            continue

        list_kind = "ul" if _RE_UL.match(stripped) else "ol" if _RE_OL.match(stripped) else None
        if list_kind:
            pattern = _RE_UL if list_kind == "ul" else _RE_OL
            items: list[list[str]] = []
            while i < n:
                cur = lines[i]
                cur_stripped = cur.strip()
                m = pattern.match(cur_stripped)
                if m:
                    items.append([m.group(1)])
                    i += 1
                    continue
                # Продолжение пункта: непустая строка с отступом.
                if cur_stripped and cur.startswith((" ", "\t")) and items:
                    items[-1].append(cur_stripped)
                    i += 1
                    continue
                break
            nodes.append(
                _tag(list_kind, [_tag("li", _paragraph_nodes(item, resolve_link)) for item in items])
            )
            continue

        para: list[str] = []
        while i < n:
            cur = lines[i]
            cur_stripped = cur.strip()
            if not cur_stripped:
                break
            if (
                _RE_HEADING.match(cur_stripped)
                or _RE_FENCE.match(cur_stripped)
                or _RE_HR.match(cur_stripped)
                or _RE_UL.match(cur_stripped)
                or _RE_OL.match(cur_stripped)
                or cur_stripped.startswith(">")
                or _RE_IMAGE_LINE.match(cur_stripped)
            ):
                break
            para.append(cur)
            i += 1
        nodes.append(_tag("p", _paragraph_nodes(para, resolve_link)))

    return ParsedPage(title=title, nodes=nodes)


def content_size(nodes: list[Node]) -> int:
    return len(json.dumps(nodes, ensure_ascii=False).encode("utf-8"))


# ---------------------------------------------------------------------------
# Реестр страниц (pages.json)
# ---------------------------------------------------------------------------

GUIDE_SCHEME = "guide:"


@dataclass
class PageEntry:
    id: str
    file: str
    setting: Optional[str]
    path: Optional[str]
    url: Optional[str]
    title: str = ""
    nodes: list[Node] = field(default_factory=list)
    unresolved: list[str] = field(default_factory=list)


@dataclass
class Registry:
    author_name: str
    author_url: str
    pages: list[PageEntry]
    raw: dict[str, Any]
    source: Path

    @classmethod
    def load(cls, path: Path) -> "Registry":
        raw = json.loads(path.read_text(encoding="utf-8"))
        pages = [
            PageEntry(
                id=p["id"],
                file=p["file"],
                setting=p.get("setting"),
                path=p.get("path"),
                url=p.get("url"),
            )
            for p in raw["pages"]
        ]
        ids = [p.id for p in pages]
        dupes = {x for x in ids if ids.count(x) > 1}
        if dupes:
            raise SystemExit(f"pages.json: повторяются id {sorted(dupes)}")
        return cls(
            author_name=raw.get("author_name", ""),
            author_url=raw.get("author_url", ""),
            pages=pages,
            raw=raw,
            source=path,
        )

    def by_id(self, page_id: str) -> Optional[PageEntry]:
        for p in self.pages:
            if p.id == page_id:
                return p
        return None

    def url_of(self, page_id: str) -> Optional[str]:
        p = self.by_id(page_id)
        if p is None:
            return None
        if p.url:
            return p.url
        if p.path:
            return f"https://telegra.ph/{p.path}"
        return None

    def save(self) -> None:
        """Пишет назад ТОЛЬКО path/url: остальные поля сохраняются как были,
        чтобы ручные правки pages.json не терялись."""
        by_id = {p.id: p for p in self.pages}
        for entry in self.raw["pages"]:
            p = by_id[entry["id"]]
            entry["path"] = p.path
            entry["url"] = p.url
        self.source.write_text(
            json.dumps(self.raw, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
        )


def make_resolver(registry: Registry, page: PageEntry) -> LinkResolver:
    """`guide:<id>` -> URL страницы. Неизвестный или ещё не созданный id
    остаётся как есть и попадает в page.unresolved."""

    def resolve(href: str) -> Optional[str]:
        if not href.startswith(GUIDE_SCHEME):
            return None
        target = href[len(GUIDE_SCHEME) :]
        url = registry.url_of(target)
        if url is None:
            if target not in page.unresolved:
                page.unresolved.append(target)
            return None
        return url

    return resolve


def render_page(registry: Registry, page: PageEntry, base_dir: Path) -> None:
    text = (base_dir / page.file).read_text(encoding="utf-8")
    page.unresolved = []
    parsed = parse_markdown(text, make_resolver(registry, page))
    if not parsed.title:
        raise SystemExit(f"{page.file}: нет заголовка первого уровня (# ...), он нужен как title страницы")
    page.title = parsed.title
    page.nodes = parsed.nodes


# ---------------------------------------------------------------------------
# Telegraph API
# ---------------------------------------------------------------------------


class TelegraphError(RuntimeError):
    pass


class Telegraph:
    """Минимальный клиент: только createPage и editPage.

    Токен хранится в объекте и уходит только в тело POST-запроса; в URL он не
    попадает, поэтому ни одна ошибка urllib его не показывает.
    """

    def __init__(self, token: str, author_name: str, author_url: str):
        self._token = token
        self.author_name = author_name
        self.author_url = author_url

    def _call(self, method: str, fields: dict[str, Any]) -> dict[str, Any]:
        body = urllib.parse.urlencode({"access_token": self._token, **fields}).encode("utf-8")
        req = urllib.request.Request(
            f"{API_BASE}/{method}",
            data=body,
            headers={"Content-Type": "application/x-www-form-urlencoded"},
            method="POST",
        )
        try:
            with urllib.request.urlopen(req, timeout=30) as resp:
                payload = json.loads(resp.read().decode("utf-8"))
        except urllib.error.HTTPError as e:
            raise TelegraphError(f"{method.split('/')[0]}: HTTP {e.code}") from None
        except urllib.error.URLError as e:
            raise TelegraphError(f"{method.split('/')[0]}: сеть: {e.reason}") from None
        if not payload.get("ok"):
            raise TelegraphError(f"{method.split('/')[0]}: {payload.get('error', 'неизвестная ошибка')}")
        return payload["result"]

    def _page_fields(self, title: str, nodes: list[Node]) -> dict[str, Any]:
        return {
            "title": title,
            "author_name": self.author_name,
            "author_url": self.author_url,
            "content": json.dumps(nodes, ensure_ascii=False),
            "return_content": "false",
        }

    def create_page(self, title: str, nodes: list[Node]) -> dict[str, Any]:
        return self._call("createPage", self._page_fields(title, nodes))

    def edit_page(self, path: str, title: str, nodes: list[Node]) -> dict[str, Any]:
        return self._call(f"editPage/{path}", self._page_fields(title, nodes))


# ---------------------------------------------------------------------------
# Сценарий
# ---------------------------------------------------------------------------


def select_pages(registry: Registry, only: list[str]) -> list[PageEntry]:
    if not only:
        return list(registry.pages)
    missing = [x for x in only if registry.by_id(x) is None]
    if missing:
        raise SystemExit(f"неизвестные id в --only: {missing}; есть: {[p.id for p in registry.pages]}")
    return [p for p in registry.pages if p.id in only]


def check_sizes(pages: list[PageEntry]) -> list[str]:
    problems = []
    for p in pages:
        size = content_size(p.nodes)
        if size > CONTENT_LIMIT_BYTES:
            problems.append(f"{p.id}: content {size} байт > лимита {CONTENT_LIMIT_BYTES}")
    return problems


def dry_run(registry: Registry, pages: list[PageEntry], base_dir: Path, print_nodes: bool) -> int:
    rc = 0
    for p in pages:
        render_page(registry, p, base_dir)
        action = "editPage" if p.path else "createPage"
        size = content_size(p.nodes)
        print(f"== {p.id}: {action} · «{p.title}» · узлов: {len(p.nodes)} · {size} байт · файл {p.file}")
        if p.path:
            print(f"   path: {p.path}")
        if p.setting:
            print(f"   setting: {p.setting}")
        if p.unresolved:
            # В dry-run это не ошибка: новые страницы ещё не созданы. Но
            # ссылка на несуществующий id - ошибка всегда.
            unknown = [x for x in p.unresolved if registry.by_id(x) is None]
            pending = [x for x in p.unresolved if registry.by_id(x) is not None]
            if pending:
                print(f"   ссылки на ещё не созданные страницы (заполнятся при публикации): {pending}")
            if unknown:
                print(f"   ОШИБКА: ссылки на неизвестные id: {unknown}")
                rc = 1
        if print_nodes:
            print(json.dumps(p.nodes, ensure_ascii=False, indent=1))
    for problem in check_sizes(pages):
        print(f"ОШИБКА: {problem}")
        rc = 1
    print(f"\nВсего страниц: {len(pages)}; запросов в сеть не было.")
    return rc


def publish(registry: Registry, pages: list[PageEntry], base_dir: Path, token: str) -> int:
    api = Telegraph(token, registry.author_name, registry.author_url)

    # Проход 1: завести страницы без path заглушкой, чтобы их URL уже можно
    # было подставить в ссылки остальных. pages.json сохраняется после каждой,
    # иначе обрыв между двумя createPage оставил бы страницу-сироту в Telegraph.
    for p in pages:
        if p.path:
            continue
        render_page(registry, p, base_dir)  # ради title
        result = api.create_page(p.title, [_tag("p", [PLACEHOLDER_TEXT])])
        p.path = result["path"]
        p.url = result.get("url") or f"https://telegra.ph/{p.path}"
        registry.save()
        print(f"создана {p.id} -> {p.url}")
        time.sleep(0.3)

    # Проход 2: полное содержимое на все выбранные страницы.
    for p in pages:
        render_page(registry, p, base_dir)
    unresolved = {p.id: p.unresolved for p in pages if p.unresolved}
    if unresolved:
        print(f"ОШИБКА: не разрешённые ссылки guide:<id>: {unresolved}")
        print("Подсказка: страницы из ссылок ещё не созданы, запустите без --only.")
        return 1
    problems = check_sizes(pages)
    if problems:
        for problem in problems:
            print(f"ОШИБКА: {problem}")
        return 1

    for p in pages:
        result = api.edit_page(p.path, p.title, p.nodes)
        p.url = result.get("url") or p.url or f"https://telegra.ph/{p.path}"
        registry.save()
        print(f"{p.id} -> {p.url}")
        time.sleep(0.3)

    print("\nНастройки бота (Settings -> Guides), если ссылка изменилась или ключ новый:")
    for p in registry.pages:
        if p.setting and p.url:
            print(f"  {p.setting} = {p.url}")
    return 0


def main(argv: Optional[list[str]] = None) -> int:
    parser = argparse.ArgumentParser(description="Публикация docs/guides в Telegraph.")
    parser.add_argument(
        "--pages",
        default=None,
        help="путь к pages.json (по умолчанию docs/guides/pages.json рядом с репозиторием)",
    )
    parser.add_argument("--dry-run", action="store_true", help="напечатать узлы, в сеть не ходить")
    parser.add_argument(
        "--summary",
        action="store_true",
        help="в --dry-run печатать только сводку без JSON узлов",
    )
    parser.add_argument("--only", nargs="+", default=[], metavar="ID", help="только эти страницы")
    args = parser.parse_args(argv)

    repo_root = Path(__file__).resolve().parent.parent
    pages_path = Path(args.pages) if args.pages else repo_root / "docs" / "guides" / "pages.json"
    if not pages_path.exists():
        print(f"нет файла {pages_path}", file=sys.stderr)
        return 2
    registry = Registry.load(pages_path)
    base_dir = pages_path.parent
    pages = select_pages(registry, args.only)

    if args.dry_run:
        return dry_run(registry, pages, base_dir, print_nodes=not args.summary)

    token = os.environ.get(TOKEN_ENV, "").strip()
    if not token:
        print(
            f"нет токена: задайте переменную окружения {TOKEN_ENV} (или используйте --dry-run)",
            file=sys.stderr,
        )
        return 2
    try:
        return publish(registry, pages, base_dir, token)
    except TelegraphError as e:
        print(f"Telegraph: {e}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
