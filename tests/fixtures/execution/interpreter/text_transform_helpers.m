a = erase('banana', "na");
b = replace("banana", 'na', "NA");
c = strcat('mat', 'lab');
d = strcat("mat", 'lab', "!");
e = split('a,b,c', ',');
f = split("alpha-beta", '-');
g = replace(erase("cabana", "ba"), 'a', "A");
h = split(["alpha-beta"; "gamma-delta"], '-');
i = splitlines("alpha\nbeta\ngamma");
j = split("alpha,beta;gamma", ["," ";"]);
[k, l] = split("alpha,beta;gamma", ["," ";"]);
m = split({'alpha-beta'; 'gamma-delta'}, '-');
n = replace("foo bar baz", ["foo" "baz"], ["FOO" "BAZ"]);
o = replace(["apple-pear"; "pear-apple"], ["apple" "pear"], "fruit");
p = erase("foo bar baz", ["foo " " baz"]);
q = split({'alpha-beta'; "gamma-delta"}, '-');
r = strrep({'banana', 'cabana'}, 'ba', 'BA');
s = strrep("abc 2 def 22 ghi 222 jkl 2222", '22', '*');
try
  split(['ab'; 'cd']);
catch err
  t = err.identifier;
  u = err.message;
end
try
  splitlines(['ab'; 'cd']);
catch err2
  v = err2.identifier;
  w = err2.message;
end
