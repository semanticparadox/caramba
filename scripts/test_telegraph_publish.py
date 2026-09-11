"""Юнит-тесты конвертера Markdown -> узлы Telegraph (scripts/telegraph-publish.py).

Запуск из корня репозитория:
  python3 -m unittest scripts/test_telegraph_publish.py
или из scripts/:
  python3 -m unittest

Сеть не нужна: проверяется только чистая часть (разбор, реестр, разрешение
ссылок, защита токена).
"""

from __future__ import annotations

import importlib.util
import json
import os
import sys
import tempfile
import unittest
from pathlib import Path

_HERE = Path(__file__).resolve().parent
_SPEC = importlib.util.spec_from_file_location("telegraph_publish", _HERE / "telegraph-publish.py")
tp = importlib.util.module_from_spec(_SPEC)
assert _SPEC.loader is not None
# dataclasses при отложенных аннотациях (from __future__ import annotations)
# ищут модуль в sys.modules, а у файла с дефисом в имени обычного импорта нет.
sys.modules[_SPEC.name] = tp
_SPEC.loader.exec_module(tp)


def p(text: str):
    return tp.parse_markdown(text)


class InlineTests(unittest.TestCase):
    def test_plain_text(self):
        self.assertEqual(tp.render_inline("просто текст"), ["просто текст"])

    def test_bold_italic_code(self):
        nodes = tp.render_inline("a **b** *c* _d_ `e`")
        self.assertEqual(
            nodes,
            [
                "a ",
                {"tag": "strong", "children": ["b"]},
                " ",
                {"tag": "em", "children": ["c"]},
                " ",
                {"tag": "em", "children": ["d"]},
                " ",
                {"tag": "code", "children": ["e"]},
            ],
        )

    def test_nested_bold_inside_link(self):
        nodes = tp.render_inline("[**жирная** ссылка](https://x.y/z)")
        self.assertEqual(
            nodes,
            [
                {
                    "tag": "a",
                    "attrs": {"href": "https://x.y/z"},
                    "children": [{"tag": "strong", "children": ["жирная"]}, " ссылка"],
                }
            ],
        )

    def test_code_is_not_parsed_inside(self):
        nodes = tp.render_inline("`**не жирный** [не ссылка](x)`")
        self.assertEqual(nodes, [{"tag": "code", "children": ["**не жирный** [не ссылка](x)"]}])

    def test_escapes(self):
        self.assertEqual(tp.render_inline(r"\*звёздочка\* и \_подчёркивание\_"), ["*звёздочка* и _подчёркивание_"])

    def test_underscore_inside_word_is_literal(self):
        self.assertEqual(tp.render_inline("guide_url_index и snake_case"), ["guide_url_index и snake_case"])

    def test_asterisk_alone_is_literal(self):
        self.assertEqual(tp.render_inline("2 * 2 = 4"), ["2 * 2 = 4"])

    def test_inline_image(self):
        nodes = tp.render_inline("см. ![схема](https://i/x.png) тут")
        self.assertEqual(
            nodes,
            ["см. ", {"tag": "img", "attrs": {"src": "https://i/x.png", "alt": "схема"}}, " тут"],
        )

    def test_link_resolver_is_applied(self):
        resolver = lambda href: "https://telegra.ph/Faq-01-01" if href == "guide:faq" else None
        nodes = tp.render_inline("[FAQ](guide:faq) и [внешняя](https://a.b)", resolver)
        self.assertEqual(nodes[0]["attrs"]["href"], "https://telegra.ph/Faq-01-01")
        self.assertEqual(nodes[2]["attrs"]["href"], "https://a.b")

    def test_unresolved_link_keeps_href(self):
        nodes = tp.render_inline("[x](guide:nope)", lambda href: None)
        self.assertEqual(nodes[0]["attrs"]["href"], "guide:nope")


class BlockTests(unittest.TestCase):
    def test_title_from_h1_not_in_content(self):
        page = p("# Заголовок страницы\n\nАбзац.")
        self.assertEqual(page.title, "Заголовок страницы")
        self.assertEqual(page.nodes, [{"tag": "p", "children": ["Абзац."]}])

    def test_headings_map_to_h3_h4(self):
        page = p("# T\n\n## Раздел\n\n### Подраздел\n\n#### Глубже")
        self.assertEqual([n["tag"] for n in page.nodes], ["h3", "h4", "h4"])
        self.assertEqual(page.nodes[0]["children"], ["Раздел"])

    def test_second_h1_becomes_h3(self):
        page = p("# T\n\n# Ещё один")
        self.assertEqual(page.nodes, [{"tag": "h3", "children": ["Ещё один"]}])

    def test_paragraph_joins_lines_and_hard_breaks(self):
        page = p("# T\n\nпервая\nвторая  \nтретья\\\nчетвёртая")
        self.assertEqual(
            page.nodes,
            [{"tag": "p", "children": ["первая вторая", {"tag": "br"}, "третья", {"tag": "br"}, "четвёртая"]}],
        )

    def test_unordered_list_with_continuation(self):
        page = p("# T\n\n- один\n- два **жирных**\n  продолжение\n* три")
        self.assertEqual(
            page.nodes,
            [
                {
                    "tag": "ul",
                    "children": [
                        {"tag": "li", "children": ["один"]},
                        {"tag": "li", "children": ["два ", {"tag": "strong", "children": ["жирных"]}, " продолжение"]},
                        {"tag": "li", "children": ["три"]},
                    ],
                }
            ],
        )

    def test_ordered_list(self):
        page = p("# T\n\n1. раз\n2. два\n3) три")
        self.assertEqual(page.nodes[0]["tag"], "ol")
        self.assertEqual([li["children"] for li in page.nodes[0]["children"]], [["раз"], ["два"], ["три"]])

    def test_list_then_paragraph_are_separate_blocks(self):
        page = p("# T\n\n- пункт\n\nабзац")
        self.assertEqual([n["tag"] for n in page.nodes], ["ul", "p"])

    def test_blockquote_hr_fence(self):
        page = p("# T\n\n> цитата\n> вторая\n\n---\n\n```\ncode 1\n  code 2\n```")
        self.assertEqual(
            page.nodes,
            [
                {"tag": "blockquote", "children": ["цитата вторая"]},
                {"tag": "hr"},
                {"tag": "pre", "children": ["code 1\n  code 2"]},
            ],
        )

    def test_fence_content_is_verbatim(self):
        page = p("# T\n\n```\n**not bold** [x](y)\n```")
        self.assertEqual(page.nodes[0]["children"], ["**not bold** [x](y)"])

    def test_image_line_becomes_figure(self):
        page = p("# T\n\n![Подпись](https://i/x.png)\n\n![](https://i/y.png)")
        self.assertEqual(
            page.nodes,
            [
                {
                    "tag": "figure",
                    "children": [
                        {"tag": "img", "attrs": {"src": "https://i/x.png"}},
                        {"tag": "figcaption", "children": ["Подпись"]},
                    ],
                },
                {"tag": "figure", "children": [{"tag": "img", "attrs": {"src": "https://i/y.png"}}]},
            ],
        )

    def test_heading_interrupts_paragraph(self):
        page = p("# T\n\nтекст\n## Раздел")
        self.assertEqual([n["tag"] for n in page.nodes], ["p", "h3"])

    def test_content_size_counts_utf8(self):
        nodes = [{"tag": "p", "children": ["ё"]}]
        self.assertEqual(tp.content_size(nodes), len(json.dumps(nodes, ensure_ascii=False).encode("utf-8")))

    def test_only_allowed_tags_are_emitted(self):
        md = (
            "# T\n\n## h\n\n### h4\n\nабзац **b** *i* `c` [a](https://x) ![i](https://y)\n\n"
            "- l\n\n1. o\n\n> q\n\n---\n\n```\nx\n```\n\n![f](https://z)"
        )
        allowed = {"a", "aside", "b", "blockquote", "br", "code", "em", "figcaption", "figure", "h3",
                   "h4", "hr", "i", "iframe", "img", "li", "ol", "p", "pre", "s", "strong", "u", "ul", "video"}
        seen = set()

        def walk(nodes):
            for n in nodes:
                if isinstance(n, dict):
                    seen.add(n["tag"])
                    walk(n.get("children", []))

        walk(p(md).nodes)
        self.assertTrue(seen <= allowed, seen - allowed)


class RegistryTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.base = Path(self.tmp.name)
        (self.base / "ru").mkdir()
        (self.base / "ru" / "a.md").write_text("# A\n\nСм. [B](guide:b) и [C](guide:c).", encoding="utf-8")
        (self.base / "ru" / "b.md").write_text("# B\n\nНазад к [A](guide:a).", encoding="utf-8")
        (self.base / "ru" / "c.md").write_text("# C\n\nтекст", encoding="utf-8")
        self.registry_path = self.base / "pages.json"
        self.registry_path.write_text(
            json.dumps(
                {
                    "author_name": "X",
                    "author_url": "https://t.me/x",
                    "pages": [
                        {"id": "a", "file": "ru/a.md", "setting": "guide_url_index", "path": "A-01-01", "url": "https://telegra.ph/A-01-01"},
                        {"id": "b", "file": "ru/b.md", "setting": "guide_url_b", "path": "B-01-01", "url": None},
                        {"id": "c", "file": "ru/c.md", "setting": None, "path": None, "url": None},
                    ],
                },
                ensure_ascii=False,
            ),
            encoding="utf-8",
        )

    def tearDown(self):
        self.tmp.cleanup()

    def test_links_resolve_from_url_or_path(self):
        reg = tp.Registry.load(self.registry_path)
        page = reg.by_id("a")
        tp.render_page(reg, page, self.base)
        hrefs = [n["attrs"]["href"] for n in page.nodes[0]["children"] if isinstance(n, dict)]
        self.assertEqual(hrefs, ["https://telegra.ph/B-01-01", "guide:c"])
        self.assertEqual(page.unresolved, ["c"])

    def test_created_path_resolves_after_save(self):
        reg = tp.Registry.load(self.registry_path)
        c = reg.by_id("c")
        c.path, c.url = "C-02-02", "https://telegra.ph/C-02-02"
        reg.save()
        reg2 = tp.Registry.load(self.registry_path)
        page = reg2.by_id("a")
        tp.render_page(reg2, page, self.base)
        self.assertEqual(page.unresolved, [])
        # Остальные поля pages.json не тронуты.
        raw = json.loads(self.registry_path.read_text(encoding="utf-8"))
        self.assertEqual(raw["pages"][0]["setting"], "guide_url_index")
        self.assertEqual(raw["author_name"], "X")

    def test_missing_title_is_an_error(self):
        (self.base / "ru" / "c.md").write_text("без заголовка", encoding="utf-8")
        reg = tp.Registry.load(self.registry_path)
        with self.assertRaises(SystemExit):
            tp.render_page(reg, reg.by_id("c"), self.base)

    def test_duplicate_ids_rejected(self):
        raw = json.loads(self.registry_path.read_text(encoding="utf-8"))
        raw["pages"].append(dict(raw["pages"][0]))
        self.registry_path.write_text(json.dumps(raw), encoding="utf-8")
        with self.assertRaises(SystemExit):
            tp.Registry.load(self.registry_path)

    def test_select_pages_unknown_id(self):
        reg = tp.Registry.load(self.registry_path)
        with self.assertRaises(SystemExit):
            tp.select_pages(reg, ["zzz"])
        self.assertEqual([x.id for x in tp.select_pages(reg, ["c", "a"])], ["a", "c"])

    def test_dry_run_does_not_touch_network_or_files(self):
        reg = tp.Registry.load(self.registry_path)
        before = self.registry_path.read_text(encoding="utf-8")
        import contextlib
        import io

        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            rc = tp.dry_run(reg, tp.select_pages(reg, []), self.base, print_nodes=False)
        self.assertEqual(rc, 0)
        self.assertEqual(self.registry_path.read_text(encoding="utf-8"), before)
        out = buf.getvalue()
        self.assertIn("createPage", out)
        self.assertIn("editPage", out)
        self.assertIn("запросов в сеть не было", out)

    def test_dry_run_flags_unknown_link_id(self):
        (self.base / "ru" / "c.md").write_text("# C\n\n[x](guide:unknown)", encoding="utf-8")
        reg = tp.Registry.load(self.registry_path)
        import contextlib
        import io

        with contextlib.redirect_stdout(io.StringIO()):
            rc = tp.dry_run(reg, tp.select_pages(reg, ["c"]), self.base, print_nodes=False)
        self.assertEqual(rc, 1)


class TokenSafetyTests(unittest.TestCase):
    def test_token_only_in_post_body(self):
        """Токен не должен попадать в URL: иначе он утёк бы в любую ошибку urllib."""
        captured = {}

        class FakeResp:
            def __enter__(self):
                return self

            def __exit__(self, *a):
                return False

            def read(self):
                return json.dumps({"ok": True, "result": {"path": "P", "url": "https://telegra.ph/P"}}).encode()

        def fake_urlopen(req, timeout=0):
            captured["url"] = req.full_url
            captured["body"] = req.data.decode()
            return FakeResp()

        original = tp.urllib.request.urlopen
        tp.urllib.request.urlopen = fake_urlopen
        try:
            api = tp.Telegraph("SECRET-TOKEN", "Автор", "https://t.me/x")
            result = api.edit_page("P", "Т", [{"tag": "p", "children": ["x"]}])
        finally:
            tp.urllib.request.urlopen = original
        self.assertEqual(result["path"], "P")
        self.assertNotIn("SECRET-TOKEN", captured["url"])
        self.assertIn("access_token=SECRET-TOKEN", captured["body"])
        self.assertEqual(captured["url"], "https://api.telegra.ph/editPage/P")

    def test_api_error_message_has_no_token(self):
        class FakeResp:
            def __enter__(self):
                return self

            def __exit__(self, *a):
                return False

            def read(self):
                return json.dumps({"ok": False, "error": "PAGE_ACCESS_DENIED"}).encode()

        original = tp.urllib.request.urlopen
        tp.urllib.request.urlopen = lambda req, timeout=0: FakeResp()
        try:
            api = tp.Telegraph("SECRET-TOKEN", "Автор", "https://t.me/x")
            with self.assertRaises(tp.TelegraphError) as ctx:
                api.create_page("Т", [])
        finally:
            tp.urllib.request.urlopen = original
        self.assertIn("PAGE_ACCESS_DENIED", str(ctx.exception))
        self.assertNotIn("SECRET-TOKEN", str(ctx.exception))

    def test_main_without_token_refuses(self):
        env_backup = os.environ.pop(tp.TOKEN_ENV, None)
        try:
            with tempfile.TemporaryDirectory() as d:
                pages = Path(d) / "pages.json"
                pages.write_text(json.dumps({"author_name": "", "author_url": "", "pages": []}), encoding="utf-8")
                rc = tp.main(["--pages", str(pages)])
            self.assertEqual(rc, 2)
        finally:
            if env_backup is not None:
                os.environ[tp.TOKEN_ENV] = env_backup


if __name__ == "__main__":
    unittest.main()
