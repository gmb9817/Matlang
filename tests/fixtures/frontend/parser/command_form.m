string pkg.sub folder/file.txt name-value 1:3 -flag ./tmp/file /tmp/file \tmp\file @helper ../rel name=value alpha==beta lower<=upper left~=right opt&&flag opt||flag key="two words" key='two words' value(1,2) nested{1,2} cell(1,[2,3]) matrix([1,2;3,4])
x = strcmp pkg.sub pkg.sub
y = char "delta"
z = strcmp alpha, alpha
w = pkg.helper alpha
q = cmdpkg.helper name=value
r = cmdpkg.helper key="two words"
s = cmdpkg.helper key='two words'
t = cmdpkg.helper name=... comment
value
u = cmdpkg.helper alpha...
beta
v = cmdpkg.helper alpha ... comment
beta
sign_base = 5
dotted_base = struct("helper", 5)
dotted_div = 2
aa = sign_base -1
ab = sign_base +1
ac = cmdpkg.helper -1:3
ad = cmdpkg.helper +1:3
ae = cmdpkg.helper -1ms
af = cmdpkg.helper +1ms
div_base = 8
divisor = 2
ag = div_base ./divisor
ah = div_base .\divisor
ai = cmdpkg.helper ./tmp/file
aj = div_base /divisor
ak = div_base \divisor
al = cmdpkg.helper /tmp/file
am = cmdpkg.helper \tmp\file
an = cmdpkg.helper value(1,2)
ao = cmdpkg.helper nested{1, 2}
ap = cmdpkg.helper cell(1,[2, 3])
aq = cmdpkg.helper matrix([1,2; 3,4])
ar = sign_base -(1 + 2)
as = sign_base +[1, 2]
at = dotted_base.helper -(1 + 2)
au = dotted_base.helper +[1, 2]
av = sign_base -sum([1, 2])
aw = sign_base +sum([1, 2])
ax = dotted_base.helper -sum([1, 2])
ay = dotted_base.helper +sum([1, 2])
rhs_struct = struct("value", 3, "items", [1, 2])
dotted_rhs = struct("value", 3, "items", [1, 2])
az = sign_base -rhs_struct.value
ba = sign_base +rhs_struct.items(2)
bb = dotted_base.helper -dotted_rhs.value
bc = dotted_base.helper +dotted_rhs.items(2)
bs = dotted_base.helper /dotted_div
bt = dotted_base.helper \dotted_div
bu = dotted_base.helper ./dotted_div
bv = dotted_base.helper .\dotted_div
mul_base = 6
mul_rhs = 2
bd = mul_base *mul_rhs
be = mul_base .*mul_rhs
bf = mul_base ^mul_rhs
bg = mul_base .^mul_rhs
bh = cmdpkg.helper *.txt
bi = dotted_base.helper .*dotted_div
bj = dotted_base.helper ^dotted_div
bk = dotted_base.helper .^dotted_div
bl = cmdpkg.helper "two words".txt
bm = cmdpkg.helper 'two words'.m
bn = cmdpkg.helper 'two words',suffix
bo = cmdpkg.helper key="a;b";tail
bp = cmdpkg.helper value(1,"a,b",3)
bq = cmdpkg.helper note='%literal comment text%'
br = cmdpkg.helper prefix"two words"suffix
print -dpng out_file
print -r300 -dpng out_file
print -dsvg out_file
print -f2 -dpdf out_pdf -bestfit -r0
ca = dotted_base.helper -(1 + 2)
cb = dotted_base.helper +[1, 2]
cc = dotted_base.helper -sum([1, 2])
cd = dotted_base.helper +sum([1, 2])
ce = dotted_base.helper *dotted_div
cf = dotted_base.helper .*dotted_div
cg = dotted_base.helper ^dotted_div
ch = dotted_base.helper .^dotted_div
postfix_base = 10
foo = struct("bar", 2)
foo_values = [2, 4]
foo_cells = {2}
ci = postfix_base /foo.bar
cj = postfix_base *foo.bar
ck = postfix_base ^foo.bar
cl = postfix_base ./foo.bar
cm = postfix_base .*foo.bar
cn = postfix_base .^foo.bar
co = postfix_base /foo_values(1)
cp = postfix_base *foo_values(1)
cq = postfix_base ^foo_values(1)
cr = postfix_base /foo_cells{1}
cs = postfix_base *foo_cells{1}
ct = postfix_base ^foo_cells{1}
cu = dotted_base.helper /foo.bar
cv = dotted_base.helper *foo.bar
cw = dotted_base.helper ^foo.bar
cx = dotted_base.helper ./foo.bar
cy = dotted_base.helper .*foo.bar
cz = dotted_base.helper .^foo.bar
da = dotted_base.helper /foo_values(1)
db = dotted_base.helper *foo_values(1)
dc = dotted_base.helper ^foo_values(1)
dd = dotted_base.helper /foo_cells{1}
de = dotted_base.helper *foo_cells{1}
df = dotted_base.helper ^foo_cells{1}
dg = cmdpkg.helper "two words"
dh = cmdpkg.helper "two words" "three words"
di = cmdpkg.helper 'two words'
dj = cmdpkg.helper "a,b" more
dk = cmdpkg.helper "a;b" tail
dl = cmdpkg.helper key="a""b"
dm = cmdpkg.helper alpha == beta
dn = cmdpkg.helper alpha ~= beta
do = cmdpkg.helper alpha <= beta
dp = cmdpkg.helper alpha >= beta
dq = cmdpkg.helper alpha < beta
dr = cmdpkg.helper alpha > beta
ds = cmdpkg.helper alpha && beta
dt = cmdpkg.helper alpha || beta
