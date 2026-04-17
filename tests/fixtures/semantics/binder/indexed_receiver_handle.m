function y = indexed_handle(objs)
f = @objs(:,2).total;
g = @objs.child(1:2).total;
y = f;
end
