"""Render every generated path, including label curves, and scan the resulting images.
Requires the validation dependencies plus cairosvg.
"""
import html
import io
from pathlib import Path
import cairosvg
from PIL import Image
import zxingcpp
from validate_wasm import ROOT, CASES, invoke


def vector_svg(out):
    bg = next(o for o in out['objects'] if o.get('name') == 'Barcode - white quiet zone')
    a = bg['path']['subpaths'][0]['anchors']
    x0,y0 = a[0]['p']
    x1,y1 = a[2]['p']
    paths=[]
    for o in out['objects']:
        if 'path' not in o:
            continue
        d=[]
        for sub in o['path']['subpaths']:
            anchors=sub['anchors']
            d.append('M'+','.join(map(str,anchors[0]['p'])))
            for previous,current in zip(anchors, anchors[1:]+anchors[:1]):
                if 'out' in previous or 'in' in current:
                    controls=[previous.get('out',previous['p']),current.get('in',current['p']),current['p']]
                    d.append('C'+' '.join(','.join(map(str,p)) for p in controls))
                else:
                    d.append('L'+','.join(map(str,current['p'])))
            d.append('Z')
        k=o['fills'][0]['color']['k']
        paths.append(f'<path fill="{"black" if k else "white"}" fill-rule="nonzero" d="{" ".join(d)}"/>')
    return x0,y0,x1-x0,y1-y0,''.join(paths)


def main():
    cells=[]
    seen=set()
    for kind,data,expected in CASES:
        if kind in seen:
            continue
        seen.add(kind)
        out,_=invoke({'symbology':kind,'data':data})
        x,y,w,h,paths=vector_svg(out)
        svg=f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="{x} {y} {w} {h}">{paths}</svg>'
        png=cairosvg.svg2png(bytestring=svg.encode(),output_width=round(w*6),output_height=round(h*6))
        decoded=zxingcpp.read_barcode(Image.open(io.BytesIO(png)))
        assert decoded and decoded.valid,kind
        col=(len(cells)%2)*400
        row=(len(cells)//2)*200
        scale=min(370/w,150/h)
        tx=col+(400-w*scale)/2-x*scale
        ty=row+36-y*scale
        cells.append(f'<text x="{col+20}" y="{row+24}" font-family="sans-serif" font-size="15" fill="#242b37">{html.escape(kind)}</text><g transform="translate({tx},{ty}) scale({scale})">{paths}</g>')
    height=((len(cells)+1)//2)*200
    svg=f'<svg xmlns="http://www.w3.org/2000/svg" width="800" height="{height}" viewBox="0 0 800 {height}"><rect width="800" height="{height}" fill="white"/>{"".join(cells)}</svg>'
    (ROOT/'assets/examples.svg').write_text(svg)
    cairosvg.svg2png(bytestring=svg.encode(),write_to=str(ROOT/'assets/examples.png'))
    print(f'{len(cells)} complete vector renders, including outlined labels, decoded successfully.')

if __name__=='__main__':
    main()
