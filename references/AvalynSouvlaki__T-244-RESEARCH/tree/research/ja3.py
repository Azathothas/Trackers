import socket, struct, hashlib, sys, threading

GREASE = {0x0a0a,0x1a1a,0x2a2a,0x3a3a,0x4a4a,0x5a5a,0x6a6a,0x7a7a,
          0x8a8a,0x9a9a,0xaaaa,0xbaba,0xcaca,0xdada,0xeaea,0xfafa}

def parse_ch(data):
    # TLS record: type(1) ver(2) len(2)
    assert data[0]==0x16, "not a handshake"
    p = 5
    assert data[p]==0x01, "not ClientHello"
    hs_len = int.from_bytes(data[p+1:p+4],'big'); p += 4
    ver = int.from_bytes(data[p:p+2],'big'); p += 2
    p += 32                                    # random
    sid_len = data[p]; p += 1 + sid_len        # session id
    cs_len = int.from_bytes(data[p:p+2],'big'); p += 2
    ciphers = [int.from_bytes(data[p+i:p+i+2],'big') for i in range(0,cs_len,2)]; p += cs_len
    comp_len = data[p]; p += 1 + comp_len
    ext_total = int.from_bytes(data[p:p+2],'big'); p += 2
    end = p + ext_total
    exts, curves, pf, alpn, sigalgs, versions = [], [], [], [], [], []
    while p < end:
        et = int.from_bytes(data[p:p+2],'big'); el = int.from_bytes(data[p+2:p+4],'big'); p += 4
        body = data[p:p+el]; p += el
        exts.append(et)
        if et == 0x000a:
            n = int.from_bytes(body[0:2],'big')
            curves = [int.from_bytes(body[2+i:4+i],'big') for i in range(0,n,2)]
        elif et == 0x000b: pf = list(body[1:])
        elif et == 0x0010:
            q=2
            while q < len(body):
                L=body[q]; alpn.append(body[q+1:q+1+L].decode()); q += 1+L
        elif et == 0x000d:
            n = int.from_bytes(body[0:2],'big')
            sigalgs = [int.from_bytes(body[2+i:4+i],'big') for i in range(0,n,2)]
        elif et == 0x002b:
            n = body[0]
            versions = [int.from_bytes(body[1+i:3+i],'big') for i in range(0,n,2)]
    return dict(ver=ver, ciphers=ciphers, exts=exts, curves=curves, pf=pf,
                alpn=alpn, sigalgs=sigalgs, versions=versions)

def ja3(d):
    f = lambda xs: '-'.join(str(x) for x in xs if x not in GREASE)
    s = f"{d['ver']},{f(d['ciphers'])},{f(d['exts'])},{f(d['curves'])},{'-'.join(map(str,d['pf']))}"
    return s, hashlib.md5(s.encode()).hexdigest()

def ja4(d):
    ng = lambda xs: [x for x in xs if x not in GREASE]
    v = max([x for x in ng(d['versions'])] or [d['ver']])
    vs = {0x0304:'13',0x0303:'12',0x0302:'11',0x0301:'10'}.get(v,'00')
    alp = (d['alpn'][0][0]+d['alpn'][0][-1]) if d['alpn'] else '00'
    cs, ex = ng(d['ciphers']), [e for e in ng(d['exts']) if e not in (0x0000,0x0010)]
    a = f"t{vs}d{len(cs):02d}{len(ng(d['exts'])):02d}{alp}"
    b = hashlib.sha256(','.join(f'{c:04x}' for c in sorted(cs)).encode()).hexdigest()[:12]
    c = hashlib.sha256((','.join(f'{e:04x}' for e in sorted(ex))+'_'+
                        ','.join(f'{s:04x}' for s in ng(d['sigalgs']))).encode()).hexdigest()[:12]
    return f"{a}_{b}_{c}"

def serve(port):
    s = socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR,1)
    s.bind(('127.0.0.1',port)); s.listen(5)
    print(f"listening :{port}", flush=True)
    while True:
        c,_ = s.accept()
        try:
            data = c.recv(16384)
            if data and data[0]==0x16:
                d = parse_ch(data)
                st, h = ja3(d)
                print("="*70)
                print("JA3   :", h)
                print("JA3str:", st[:160]+("..." if len(st)>160 else ""))
                print("JA4   :", ja4(d))
                print("ciphers  :", len(d['ciphers']), "(GREASE:", sum(1 for x in d['ciphers'] if x in GREASE),")")
                print("ext order:", '-'.join(hex(e) for e in d['exts']))
                print("ALPN     :", d['alpn'])
                print("curves   :", [hex(x) for x in d['curves']])
                print("TLS vers :", [hex(x) for x in d['versions']])
                print("="*70, flush=True)
        except Exception as e: print("ERR", e, flush=True)
        finally: c.close()

if __name__ == "__main__": serve(int(sys.argv[1]))
