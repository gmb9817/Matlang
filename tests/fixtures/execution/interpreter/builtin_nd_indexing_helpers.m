a = cat(3, [1 2; 3 4], [5 6; 7 8]);
b = size(a);
c = ndims(a);
d = a(1, 2, 2);
e = a(2, end, 1);
f = a(:, :, 2);
g = a(1, :, :);
h = size(g);

i = cat(3, {1, 2; 3, 4}, {5, 6; 7, 8});
j = i{2, 1, 2};
k = i(:, :, 2);
l = size(k);

a(1, 1, 3) = 9;
m = size(a);
n = a(1, 1, 3);
o = a(:, :, 3);

i{1, 2, 3} = 60;
p = i{1, 2, 3};
q = i(:, :, 3);
r = size(i);

s = cat(3, [1 2; 3 4]);
s(:, 3) = [5; 6];
t = size(s);

u = cat(3, {1, 2; 3, 4});
u(:, 3) = {5; 6};
v = size(u);

w = cat(3, [1; 2; 3]);
w(4) = 4;
x = size(w);

y = cat(3, {1; 2; 3});
y(4) = {4};
z = size(y);

aa = reshape([1 2 3 4], [2 2 1 1]);
aa(1, 1, 2) = 9;
ab = size(aa);
ac = aa(:, :, 2);

cc = reshape({1 2 3 4}, [2 2 1 1]);
cc(1, 1, 2) = {9};
cd = size(cc);
ce = cc(:, :, 2);
