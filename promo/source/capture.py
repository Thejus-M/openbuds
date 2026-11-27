import sys, asyncio
from playwright.async_api import async_playwright
FPS=30; N=1140
only=[float(x) for x in sys.argv[1:]]
async def main():
    async with async_playwright() as p:
        b=await p.chromium.launch(executable_path="/usr/bin/chromium",args=["--allow-file-access-from-files"])
        pg=await b.new_page(viewport={"width":1080,"height":1920})
        await pg.goto("file://"+__import__('os').path.abspath("video.html"))
        await pg.evaluate("document.fonts.ready")
        ts=[(f"still-{t:05.2f}.png",t) for t in only] if only else [(f"frames/f{i:04d}.png",i/FPS) for i in range(N)]
        for name,t in ts:
            await pg.evaluate(f"render({t})"); await pg.screenshot(path=name)
        await b.close()
asyncio.run(main())
