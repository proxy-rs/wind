use crate::AbstractTcpStream;

pub trait AbstractOutbound {
    /// TCP traffic which needs handled by outbound
    fn listen(
       &self,
    ) -> impl Future<Output = ()> + Send;
 }