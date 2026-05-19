a = struct("name", {}, "value", {});
b = a';
bc = class(b);
bf = fieldnames(b);
c = [b b];
cc = class(c);
cf = fieldnames(c);
