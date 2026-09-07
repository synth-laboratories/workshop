"""Materialize explicitly selected source lines; never guess or rewrite code."""
def numbered(text):
    return '\n'.join(f'[{i}] {line}' for i,line in enumerate(text.splitlines(),1))

def extract(text, span):
    if not isinstance(span,dict):return None
    start,end=span.get('start'),span.get('end')
    if type(start) is not int or type(end) is not int:return None
    lines=text.splitlines(keepends=True)
    if not 1<=start<=end<=len(lines) or end-start>=24:return None
    quote=''.join(lines[start-1:end]).rstrip('\r\n')
    return quote if quote.strip() and len(quote)<=2400 else None
