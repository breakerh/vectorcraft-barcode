"""Validate the shipped WASM and independently decode vector bar geometry with ZXing.

Run: python -m pip install wasmtime pillow zxing-cpp
     python tests/validate_wasm.py
"""
import argparse
import json
from pathlib import Path
import wasmtime
from PIL import Image, ImageDraw
import zxingcpp

ROOT = Path(__file__).resolve().parents[1]
config = wasmtime.Config()
config.consume_fuel = True
engine = wasmtime.Engine(config)
module = wasmtime.Module.from_file(engine, str(ROOT / 'vectorcraft-barcode.wasm'))
assert not module.imports, 'The plugin must not import host capabilities.'


def invoke(params, selected=True):
    store = wasmtime.Store(engine)
    store.set_fuel(1_000_000_000)
    exports = wasmtime.Instance(store, module, []).exports(store)
    memory = exports['memory']

    def allocate(value):
        data = json.dumps(value, ensure_ascii=False).encode()
        pointer = exports['vc_alloc'](store, len(data))
        memory.write(store, data, pointer)
        return pointer, len(data)

    original = {'objects': [{'id': 77}]} if selected else {'objects': []}
    params = {'_context': {'artboard': [0, 0, 1000, 500]}, **params}
    ip, il = allocate(original)
    pp, pl = allocate(params)
    packed = exports['vc_run'](store, ip, il, pp, pl)
    assert packed >= 0
    pointer, length = packed & 0xffffffff, packed >> 32
    result = json.loads(bytes(memory.read(store, pointer, pointer + length)))
    return result, 1_000_000_000 - store.get_fuel()


def render_bars(result):
    background = next(o for o in result['objects'] if o.get('name') == 'Barcode - white quiet zone')
    anchors = background['path']['subpaths'][0]['anchors']
    x0, y0 = anchors[0]['p']
    x1, y1 = anchors[2]['p']
    scale = 8
    image = Image.new('RGB', (round((x1-x0)*scale), round((y1-y0)*scale)), 'white')
    draw = ImageDraw.Draw(image)
    bars = next(o for o in result['objects'] if o.get('name', '').startswith('Barcode - ') and o.get('name') not in ['Barcode - white quiet zone', 'Barcode - outlined text'])
    for path in bars['path']['subpaths']:
        a = path['anchors']
        left, top = a[0]['p']
        right, bottom = a[2]['p']
        draw.rectangle((round((left-x0)*scale), round((top-y0)*scale), round((right-x0)*scale)-1, round((bottom-y0)*scale)-1), fill='black')
    return image


# Only neutral, fictional identifiers or published standard examples belong here.
CASES = [
    ('EAN-13', '590123412345', '5901234123457'),
    ('EAN-13', '5901234123457', '5901234123457'),
    ('EAN-8', '9638507', '96385074'),
    ('EAN-8', '96385074', '96385074'),
    ('UPC-A', '03600029145', '036000291452'),
    ('UPC-A', '036000291452', '036000291452'),
    ('UPC-E', '425261', '04252614'),
    ('UPC-E', '04252614', '04252614'),
    ('UPC-E', '123453', None),
    ('UPC-E', '123454', None),
    ('UPC-E', '123455', None),
    ('UPC-E', '1425261', None),
    ('Code 128', 'EXAMPLE-12345', 'EXAMPLE-12345'),
    ('Code 128', '12345678901234567890', '12345678901234567890'),
    ('Code 128', '  Example-123  ', '  Example-123  '),
    ('Code 128', 'https://example.org/?id=123&mode=test', 'https://example.org/?id=123&mode=test'),
    ('Code 39', 'EXAMPLE-123', 'EXAMPLE-123'),
    ('Code 39', 'ABC 123-.$/+%', 'ABC 123-.$/+%'),
    ('Code 93', 'EXAMPLE-123', 'EXAMPLE-123'),
    ('Code 93', 'ABC 123-.$/+%', 'ABC 123-.$/+%'),
    ('ITF', '1234567890', '1234567890'),
    ('ITF-14', '1001234500001', '10012345000017'),
    ('ITF-14', '10012345000017', '10012345000017'),
    ('Codabar', '12345', 'A12345A'),
    ('Codabar', 'B12345D', 'B12345D'),
    ('Codabar', '12-34$56:78/90.12+34', 'A12-34$56:78/90.12+34A'),
    ('ISBN-13', '978030640615', '9780306406157'),
]

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--write-report', action='store_true')
    args = parser.parse_args()
    reports = []
    for kind, data, expected in CASES:
        for show_text in [True, False]:
            out, fuel = invoke({'symbology': kind, 'data': data, 'showText': show_text})
            assert 'error' not in out, (kind, data, out)
            assert out['objects'][0] == {'id': 77}
            bars = next(o for o in out['objects'] if o.get('name', '').startswith(f'Barcode - {kind} - '))
            encoded_text = bars['name'].split(' - ', 2)[2]
            expected = expected or encoded_text
            image = render_bars(out)
            decoded = zxingcpp.read_barcode(image, return_errors=True)
            assert decoded and decoded.valid, (kind, data, decoded)
            actual = decoded.text
            # ZXing may normalize UPC-A to EAN-13 or UPC-E to expanded UPC-A.
            candidates = {expected}
            if kind == 'UPC-A':
                candidates.add('0' + expected)
            if kind == 'UPC-E':
                ns, d, check = expected[0], expected[1:7], expected[7]
                if d[5] in '012':
                    expanded = ns + d[:2] + d[5] + '0000' + d[2:5] + check
                elif d[5] == '3':
                    expanded = ns + d[:3] + '00000' + d[3:5] + check
                elif d[5] == '4':
                    expanded = ns + d[:4] + '00000' + d[4] + check
                else:
                    expanded = ns + d[:5] + '0000' + d[5] + check
                candidates.update([expanded, '0' + expanded])
            if kind == 'Codabar':
                candidates.add(expected[1:-1])
            assert actual in candidates, (kind, data, expected, actual, str(decoded.format))
            if show_text:
                labels = [o for o in out['objects'] if o.get('name') == 'Barcode - outlined text']
                assert len(labels) == 1 and labels[0]['path']['subpaths']
                assert any('in' in a or 'out' in a for p in labels[0]['path']['subpaths'] for a in p['anchors'])
            reports.append({'type': kind, 'input': data, 'expected': expected, 'decoded': actual, 'format': str(decoded.format), 'outlinedText': show_text, 'fuel': fuel, 'passed': True})
            print(f'{kind}: {data!r} -> {actual!r}, text={show_text}, fuel={fuel:,}')

    errors = [
        ('EAN-13', '5901234123450'), ('EAN-8', '96385070'), ('UPC-A', '036000291450'),
        ('UPC-E', '04252615'), ('ISBN-13', '5901234123457'), ('ITF-14', '10012345000010'),
        ('Code 128', 'café'), ('Code 39', 'lowercase'), ('ITF', '123'),
        ('Codabar', 'A123'), ('UPC-E', '2425261'), ('EAN-13', '123 456'),
        ('Code 128', ''), ('Code 128', 'X'*81), ('Code 128', 'hello\nworld'),
    ]
    for kind, data in errors:
        out, _ = invoke({'symbology': kind, 'data': data})
        assert set(out) == {'error'}, (kind, data, out)
    for kind, _, _ in CASES:
        data = next(d for k,d,_ in CASES if k == kind)
        out, _ = invoke({'symbology': kind, 'data': data, 'whiteBackground': False, 'showText': False})
        assert len(out['objects']) == 2 and out['objects'][0] == {'id':77}
    if args.write_report:
        (ROOT / 'scan-test-results.json').write_text(json.dumps({'cases':reports, 'invalidInputCasesPassed':len(errors), 'noHostImports':True}, indent=2)+'\n')
    print(f'{len(reports)} vector barcode scans passed; {len(errors)} invalid inputs rejected; no host imports.')

if __name__ == '__main__':
    main()
